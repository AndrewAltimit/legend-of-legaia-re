//! Lift the text of another Latin-script Legaia disc into a **USA-keyed
//! working pack**.
//!
//! Every Latin-script release is 1:1 with the USA disc at the container level
//! (see `docs/tooling/pal-localizations.md`): a USA PROT coordinate names the
//! same logical asset on every disc, the five SCUS name tables exist id-for-id
//! at build-shifted VAs, and the `0x1F`-segment dialog corpus pairs by position
//! within each PROT entry. This module re-keys the source disc's text onto the
//! USA coordinate space the [importer](super::import) patches. The source may be
//!
//! - one of the three **measured** official PAL localizations
//!   (`SCES_019.44`/`.45`/`.46` = FR/DE/IT), whose name-table bases are pinned;
//! - an **unmeasured** official build (`SCES_019.47` Spain, `SCES_017.52` EU
//!   English), whose bases are located by scanning out from the USA VAs;
//! - a **fan-patched** disc of any of the above - including a patched USA disc
//!   (`SCUS_942.54`) - which is how a community translation shipped as a binary
//!   patch becomes an editable pack: patch the disc it was built for, then lift.
//!
//! The Japanese builds are not lifted: their text is not the Latin codec.
//!
//! - **Name tables** (item / spell / arts / accessory / party): id-for-id. The
//!   USA pack keys each pooled string by its *USA* virtual address; the same id
//!   on the source exe points at the localized string, so the map is
//!   `usa_string_va -> source_string`. The source base is *located* (a pinned
//!   VA is verified by following its pointers, with a windowed search fallback;
//!   an unpinned build is searched from the USA VA outright), never trusted
//!   blind.
//! - **Dialog** (`man:` scene-bundle MANs, `raw:` streaming-scene MANs):
//!   structural. Each MAN is walked record by record with the field-VM
//!   disassembler and a line is keyed `(record ordinal, ordinal among that
//!   record's text leads)` - the same coordinate on every build, because the
//!   script is one program with different strings (byte offsets differ - the
//!   localized MAN repacks). A line the walk does not place falls back to the
//!   scan ordinal (the Nth qualifying segment of the entry), which is what a
//!   line the other disc's quality gate drops would otherwise shift.
//! - **String pools** (`ui:` overlay pools, `system_text` SCUS strings,
//!   `place_names` cells): NUL-chunk lists of the same window on both builds,
//!   anchored on byte-identical chunks and paired by ordinal inside each
//!   partition ([`pair_chunk_lists`]).
//!
//! The result is a **working pack** (`source:` = USA text, `translation:` =
//! official PAL text) carrying the USA per-string byte budgets. It is filled
//! with the game's copyrighted text, so it is scratchpad-only output - never
//! committed. The lifted `translation` bytes are the raw PAL bytes decoded to
//! markup (accents become `{82}`-style single-byte escapes), which the
//! [markup codec](super::markup) round-trips exactly.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

use legaia_asset::field_disasm;
use legaia_asset::{item_names, man_section, new_game, worldmap_menu};

use crate::disc::DiscPatcher;

use super::export::{SceneManText, export_pack};
use super::pack::LanguagePack;
use super::stream_man::StreamManText;
use super::{markup, segments, ui};

/// Longest string the pooled-name reader follows before calling a pointer bogus.
const MAX_STRLEN: usize = 512;

/// One SCUS name table: a pointer table with a shared record stride, plus the
/// per-record byte offsets of the string-pointer fields it owns. The USA base
/// is the pack's coordinate space; the PAL base is located per disc.
struct TableSpec {
    /// Reporting label.
    name: &'static str,
    /// USA virtual address of record 0's first pointed field.
    usa_base: u32,
    /// Record stride in bytes.
    stride: u32,
    /// Number of records to walk.
    count: u32,
    /// Pointer-field byte offsets within a record (relative to `usa_base`)
    /// whose strings the lift carries.
    fields: &'static [u32],
    /// Every pointer word in the record, lifted or not. The bytes outside
    /// these words are the record's **meta columns** (stats, ids, scope), the
    /// same on every build, and they fingerprint the base: a same-shaped
    /// alias (any item-table record read four bytes in has the accessory
    /// record's `[meta, ptr, ptr]` shape) validates as names but not as meta.
    ptr_words: &'static [u32],
}

/// The five pooled-string tables (party names are a fixed field, handled apart).
/// Bases mirror the parser constants (`item_names::TABLE_VA` etc.); fields are
/// the exact pointer offsets [`super::export`] collects.
const TABLES: &[TableSpec] = &[
    TableSpec {
        name: "items",
        usa_base: item_names::TABLE_VA, // 0x8007436C (name ptr); +4 = type ptr
        stride: 0x0C,
        count: 256,
        fields: &[0, 4],
        ptr_words: &[0, 4], // +8 = packed price / id / type
    },
    TableSpec {
        name: "spells",
        usa_base: 0x8007_54C8, // spell_names::STATS_VA; +8 = name ptr
        stride: 0x0C,
        count: 256,
        fields: &[8],
        ptr_words: &[8], // +0..+8 = stats + description index
    },
    TableSpec {
        name: "arts",
        usa_base: 0x8007_5EC4, // arts_table::TABLE_VA; +0xC = name ptr
        stride: 0x14,
        count: 256,
        fields: &[0xC],
        ptr_words: &[8, 0xC, 0x10], // +8 glyphs, +0x10 description; +0..+8 = char / index / AP
    },
    TableSpec {
        name: "accessory_passives",
        usa_base: 0x8007_625C, // accessory_passive::PASSIVE_TABLE_VA; +4 name, +8 desc
        stride: 0x0C,
        count: 0x40,
        fields: &[4, 8],
        ptr_words: &[4, 8], // +0 = scope word
    },
];

/// Per-language PAL base VAs for [`TABLES`], in table order, plus the new-game
/// party-template base. Located, not shift-computed - the pointer-table region
/// drifts locally per language (see `docs/tooling/pal-localizations.md`).
#[derive(Debug, Clone, Copy)]
struct PalBases {
    /// Pinned base of each [`TABLES`] entry, same order.
    table_bases: [u32; 4],
    /// Pinned new-game party-template base.
    party_base: u32,
}

/// Where a source build's table bases come from.
#[derive(Debug, Clone, Copy)]
enum SourceBases {
    /// A measured official localization: pinned VAs, validated then windowed.
    Pinned(PalBases),
    /// An unmeasured Latin build: every base is located by scanning out from
    /// its USA VA ([`SEARCH_BELOW`] / [`SEARCH_ABOVE`]).
    Located,
}

/// What the lift knows about a source disc from its boot exe name.
#[derive(Debug, Clone, Copy)]
pub struct SourceBuild {
    /// Default language code the lifted pack is stamped with (the CLI's
    /// `--language` overrides it - a fan patch's language is not in the exe name).
    pub lang: &'static str,
    /// Human label for reports.
    pub label: &'static str,
    bases: SourceBases,
}

/// How far below / above a table's USA VA an unpinned search looks. The three
/// measured PAL builds drift `+0x8E0..=+0xFF4`; the JP shift is `+0x1B90`; a
/// patched USA exe drifts `0`. The window covers all of those with margin.
const SEARCH_BELOW: i64 = 0x1000;
const SEARCH_ABOVE: i64 = 0x4000;

/// Boot exe name -> source build. `None` for anything that is not a Latin-script
/// retail build (the JP `SCPS_*` discs, demos): there is nothing this lift can
/// read there.
///
/// The USA exe **is** accepted: a retail-against-retail lift is the identity
/// (every `translation` equals its `source`), but a *patched* USA disc carrying
/// a fan translation lifts into the pack that reproduces it.
pub fn source_build_for_exe(exe: &str) -> Option<SourceBuild> {
    let build = |lang, label, bases| SourceBuild { lang, label, bases };
    match exe {
        "SCUS_942.54" => Some(build("en", "USA", SourceBases::Located)),
        "SCES_017.52" => Some(build("en", "Europe, English (PAL)", SourceBases::Located)),
        "SCES_019.44" => Some(build(
            "fr",
            "France (PAL)",
            SourceBases::Pinned(PalBases {
                table_bases: [0x8007_4C4C, 0x8007_5DA8, 0x8007_67A4, 0x8007_6B3C],
                party_base: 0x8007_9508,
            }),
        )),
        "SCES_019.45" => Some(build(
            "de",
            "Germany (PAL)",
            SourceBases::Pinned(PalBases {
                table_bases: [0x8007_5360, 0x8007_64BC, 0x8007_6EB8, 0x8007_7250],
                party_base: 0x8007_9C78,
            }),
        )),
        "SCES_019.46" => Some(build(
            "it",
            "Italy (PAL)",
            SourceBases::Pinned(PalBases {
                table_bases: [0x8007_5130, 0x8007_628C, 0x8007_6C88, 0x8007_7020],
                party_base: 0x8007_9A14,
            }),
        )),
        "SCES_019.47" => Some(build("es", "Spain (PAL)", SourceBases::Located)),
        _ => None,
    }
}

/// Read the little-endian pointer word at `va` in a PS-X EXE image.
fn read_ptr(exe: &[u8], va: u32) -> Option<u32> {
    let off = item_names::file_offset_for_va(exe, va)?;
    Some(u32::from_le_bytes(exe.get(off..off + 4)?.try_into().ok()?))
}

/// Read the NUL-terminated string at `va` (raw bytes, no terminator).
fn read_cstr(exe: &[u8], va: u32) -> Option<Vec<u8>> {
    let off = item_names::file_offset_for_va(exe, va)?;
    let tail = exe.get(off..)?;
    let len = tail
        .iter()
        .take(MAX_STRLEN)
        .position(|&b| b == 0)
        .filter(|&l| l > 0)?;
    Some(tail[..len].to_vec())
}

/// `true` when `bytes` reads as a pooled name string: short, and every byte is
/// a glyph (`>= 0x20`, incl. the accented high tiles) or a legal markup control
/// (`0x01` icon, `0x5E`/`0xFF` alias, `0xC0..=0xCF` 2-byte ops). A pointer that
/// lands in code / a wrong table base fails this.
fn looks_like_name(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > 128 {
        return false;
    }
    bytes
        .iter()
        .all(|&b| b >= 0x20 || b == 0x01 || markup::is_two_byte_op(b))
}

/// Fraction of the **USA-populated** records whose PAL pointer at candidate
/// `pal_base` also resolves to a name-shaped string. Validating only over ids
/// the USA table actually names makes the check count-agnostic (records past a
/// short table's real end are ignored) and language-independent (both exes use
/// the same id space). `(fraction, sample_size)`.
fn base_valid_fraction(
    usa_exe: &[u8],
    pal_exe: &[u8],
    usa_base: u32,
    pal_base: u32,
    stride: u32,
    count: u32,
    fields: &[u32],
) -> (f64, usize) {
    let mut ok = 0usize;
    let mut seen = 0usize;
    for id in 0..count {
        for &field in fields {
            let Some(usa_ptr) = read_ptr(usa_exe, usa_base + id * stride + field) else {
                continue;
            };
            if usa_ptr == 0 || !read_cstr(usa_exe, usa_ptr).is_some_and(|s| looks_like_name(&s)) {
                continue; // USA slot isn't a real name - no evidence either way
            }
            seen += 1;
            if read_ptr(pal_exe, pal_base + id * stride + field)
                .filter(|&p| p != 0)
                .and_then(|p| read_cstr(pal_exe, p))
                .is_some_and(|s| looks_like_name(&s))
            {
                ok += 1;
            }
        }
    }
    let f = if seen == 0 {
        0.0
    } else {
        ok as f64 / seen as f64
    };
    (f, seen)
}

/// Fraction of the table's **meta bytes** (every record byte outside
/// `ptr_words`) equal between the USA table and the candidate. Language- and
/// build-independent columns, so the true base scores ~1.0; a same-shaped
/// neighbour or a one-record-off alias scores like noise. Byte-wise rather
/// than record-wise so one unexpectedly shifted word cannot zero the signal.
fn meta_match_fraction(
    usa_exe: &[u8],
    src_exe: &[u8],
    usa_base: u32,
    cand: u32,
    stride: u32,
    count: u32,
    ptr_words: &[u32],
) -> f64 {
    let is_meta = |off: u32| !ptr_words.iter().any(|&p| (p..p + 4).contains(&off));
    let mut same = 0usize;
    let mut total = 0usize;
    for id in 0..count {
        let rec = id * stride;
        let (Some(u), Some(c)) = (
            item_names::file_offset_for_va(usa_exe, usa_base + rec),
            item_names::file_offset_for_va(src_exe, cand + rec),
        ) else {
            continue;
        };
        for off in (0..stride).filter(|&o| is_meta(o)) {
            let (Some(&a), Some(&b)) =
                (usa_exe.get(u + off as usize), src_exe.get(c + off as usize))
            else {
                continue;
            };
            total += 1;
            same += usize::from(a == b);
        }
    }
    if total == 0 {
        0.0
    } else {
        same as f64 / total as f64
    }
}

/// Locate a table's base on the source exe. With a pinned VA: accept it if its
/// pointers validate against the USA-populated id set, else search `+-0x2000`
/// around it. Without one: search [`SEARCH_BELOW`] / [`SEARCH_ABOVE`] around
/// the USA VA. A search keeps, among the 4-byte-aligned candidates whose
/// pointers validate, the one whose meta columns match the USA table best
/// (then the higher pointer-valid fraction, then the nearest to the USA VA).
/// Pointer validity alone is not enough: the item table's `[ptr, ptr, meta]`
/// records read as the accessory table's `[meta, ptr, ptr]` at a 4-byte
/// offset, and one record off the true base validates on every populated id
/// but the first. Returns `(base, pointer_valid_fraction)` or `None` when
/// nothing clears the threshold with a meaningful sample.
fn locate_base(
    usa_exe: &[u8],
    pal_exe: &[u8],
    spec: &TableSpec,
    pinned: Option<u32>,
) -> Option<(u32, f64)> {
    const THRESHOLD: f64 = 0.75;
    const MIN_SAMPLE: usize = 6;
    let usa_base = spec.usa_base;
    let check = |cand: u32| {
        base_valid_fraction(
            usa_exe,
            pal_exe,
            usa_base,
            cand,
            spec.stride,
            spec.count,
            spec.fields,
        )
    };
    let meta = |cand: u32| {
        meta_match_fraction(
            usa_exe,
            pal_exe,
            usa_base,
            cand,
            spec.stride,
            spec.count,
            spec.ptr_words,
        )
    };
    let (centre, lo, hi) = match pinned {
        Some(p) => {
            let (frac, n) = check(p);
            if frac >= THRESHOLD && n >= MIN_SAMPLE {
                return Some((p, frac));
            }
            (p, -0x2000i64, 0x2000i64)
        }
        None => (usa_base, -SEARCH_BELOW, SEARCH_ABOVE),
    };
    let dist = |cand: u32| (cand as i64 - usa_base as i64).abs();
    // (candidate, pointer-valid fraction, meta fraction)
    let mut best: Option<(u32, f64, f64)> = None;
    let mut d = lo;
    while d <= hi {
        let cand = (centre as i64 + d) as u32;
        d += 4;
        let (f, n) = check(cand);
        if f < THRESHOLD || n < MIN_SAMPLE {
            continue;
        }
        let m = meta(cand);
        let better = match best {
            None => true,
            Some((b, bf, bm)) => {
                m > bm || (m == bm && (f > bf || (f == bf && dist(cand) < dist(b))))
            }
        };
        if better {
            best = Some((cand, f, m));
        }
    }
    best.map(|(b, f, _)| (b, f))
}

/// Locate the new-game party template on the source exe by fingerprint: the
/// eight `u16` stats of each of the four roster records (everything but the
/// 10-byte name) must equal the USA template's, at once. A pinned VA is tried
/// first and kept even when the fingerprint fails (the three measured builds
/// were pinned by hand); an unpinned build is searched [`SEARCH_BELOW`] /
/// [`SEARCH_ABOVE`] around the USA VA at 2-byte steps, nearest hit first.
/// Returns `(base, fingerprint_matched)`.
fn locate_party_base(usa_exe: &[u8], src_exe: &[u8], pinned: Option<u32>) -> Option<(u32, bool)> {
    const STATS_LEN: usize = new_game::RECORD_STRIDE - new_game::NAME_LEN;
    let usa_va = new_game::PARTY_TEMPLATE_VA;
    let matches = |cand: u32| -> bool {
        (0..new_game::PARTY_RECORDS).all(|rec| {
            let rec_off = (rec * new_game::RECORD_STRIDE) as u32;
            let (Some(u), Some(s)) = (
                item_names::file_offset_for_va(usa_exe, usa_va + rec_off),
                item_names::file_offset_for_va(src_exe, cand + rec_off),
            ) else {
                return false;
            };
            match (usa_exe.get(u..u + STATS_LEN), src_exe.get(s..s + STATS_LEN)) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            }
        })
    };
    if let Some(p) = pinned {
        return Some((p, matches(p)));
    }
    let mut hits: Vec<u32> = Vec::new();
    let mut d = -SEARCH_BELOW;
    while d <= SEARCH_ABOVE {
        let cand = (usa_va as i64 + d) as u32;
        if matches(cand) {
            hits.push(cand);
        }
        d += 2;
    }
    hits.into_iter()
        .min_by_key(|&c| (c as i64 - usa_va as i64).abs())
        .map(|c| (c, true))
}

/// Every base the unpinned search finds on `src_exe`: the [`TABLES`] bases in
/// table order and the party template. This is the path an unmeasured build
/// takes; it is public so the measured builds can vouch for it - on a pinned
/// build it must land on the pinned VAs (`translate_lift_official_real.rs`).
#[derive(Debug, Clone)]
pub struct LocatedBases {
    /// `(table name, located base)` per [`TABLES`] entry.
    pub tables: Vec<(&'static str, Option<u32>)>,
    /// Party-template base whose stat fingerprint matches the USA template.
    pub party: Option<u32>,
}

/// Locate every table base + the party template on `src_exe` without a pin.
pub fn locate_unpinned(usa_exe: &[u8], src_exe: &[u8]) -> LocatedBases {
    let tables = TABLES
        .iter()
        .map(|spec| {
            let hit = locate_base(usa_exe, src_exe, spec, None);
            (spec.name, hit.map(|(b, _)| b))
        })
        .collect();
    let party = locate_party_base(usa_exe, src_exe, None).map(|(b, _)| b);
    LocatedBases { tables, party }
}

/// The hand-pinned bases of a measured build (`([table bases], party base)`),
/// `None` for a build that is located at run time.
pub fn pinned_bases_for_exe(exe: &str) -> Option<([u32; 4], u32)> {
    match source_build_for_exe(exe)?.bases {
        SourceBases::Pinned(p) => Some((p.table_bases, p.party_base)),
        SourceBases::Located => None,
    }
}

/// Per-table outcome for the lift report.
#[derive(Debug, Clone)]
pub struct TableStat {
    pub name: &'static str,
    pub located: bool,
    pub pal_base: u32,
    pub valid_fraction: f64,
    /// USA string VAs mapped to a PAL string (map insertions this table made).
    pub paired: usize,
}

/// How one dialog domain's lines were paired (counts only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PairStats {
    /// Lines paired **structurally**: the same `(record, ordinal)` on both
    /// discs' clean script walks ([`walk_texts`]).
    pub structural: usize,
    /// Lines the walk does not reach on the USA side (operand runs, text
    /// past a record's first decode error) or whose entry has a different
    /// record shape on the source disc, paired by scan ordinal instead.
    pub positional: usize,
    /// Of the structural pairs, how many the scan-ordinal pairing would have
    /// matched to a **different** source line - the lines an added, removed
    /// or gate-failing segment earlier in the same entry shifted.
    pub shifted: usize,
}

/// Whole-lift outcome (counts only - no text).
#[derive(Debug, Clone, Default)]
pub struct LiftReport {
    pub language: String,
    pub exe_name: String,
    /// Human label of the source build (`France (PAL)`, `USA`, ...).
    pub build_label: String,
    pub tables: Vec<TableStat>,
    /// Where the party template was read on the source exe, if anywhere.
    pub party_base: Option<u32>,
    /// Whether the template's stat bytes matched the USA fingerprint there.
    pub party_fingerprint_ok: bool,
    /// `scus:str:*` pack entries filled / left empty (no PAL string mapped).
    pub names_filled: usize,
    pub names_unmapped: usize,
    /// `scus:party:*` entries filled.
    pub party_filled: usize,
    pub party_total: usize,
    /// `man:` dialog: pack entries, order-paired (filled), unpaired.
    pub man_total: usize,
    pub man_paired: usize,
    /// `raw:` carriers: same three.
    pub raw_total: usize,
    pub raw_paired: usize,
    /// How the `man:` / `raw:` pairs were made.
    pub man_pairing: PairStats,
    pub raw_pairing: PairStats,
    /// Overlay `ui:` pool strings paired / total, and per pool.
    pub ui_paired: usize,
    pub ui_total: usize,
    pub ui_pools: Vec<PoolStat>,
    /// `system_text` SCUS strings paired / total.
    pub system_paired: usize,
    pub system_total: usize,
    /// World-map place-name cells paired / total.
    pub cells_paired: usize,
    pub cells_total: usize,
}

/// One string pool's pairing outcome.
#[derive(Debug, Clone)]
pub struct PoolStat {
    pub label: String,
    pub paired: usize,
    pub total: usize,
}

impl LiftReport {
    pub fn man_unpaired(&self) -> usize {
        self.man_total - self.man_paired
    }
    pub fn raw_unpaired(&self) -> usize {
        self.raw_total - self.raw_paired
    }
}

/// Detect the source disc's boot exe from `SYSTEM.CNF` (`BOOT = cdrom:\NAME;1`).
/// Returns the bare ISO filename (`SCES_019.45`).
pub fn boot_exe_name(patcher: &DiscPatcher) -> Result<String> {
    let cnf = patcher
        .read_named_file("SYSTEM.CNF")
        .context("SYSTEM.CNF not found in disc image")?;
    let text = String::from_utf8_lossy(&cnf);
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim().eq_ignore_ascii_case("BOOT") {
            // v = ` cdrom:\SCES_019.45;1`
            let v = v.trim();
            let after = v.rsplit(['\\', ':', '/']).next().unwrap_or(v);
            let name = after.split(';').next().unwrap_or(after).trim();
            if !name.is_empty() {
                return Ok(name.to_string());
            }
        }
    }
    bail!("no BOOT line in SYSTEM.CNF")
}

/// MAN-domain segment texts of a PROT entry, in the exact scan order the pack
/// keys follow. `allow_high` widens the gate for a PAL build's accented lines.
fn man_seg_texts(entry: &[u8], allow_high: bool) -> Vec<(usize, Vec<u8>)> {
    match SceneManText::locate(entry) {
        Some(man) => segments::scan_ext(&man.decoded, allow_high)
            .iter()
            .map(|s| {
                (
                    s.text_off,
                    man.decoded[s.text_off..s.text_off + s.len].to_vec(),
                )
            })
            .collect(),
        None => Vec::new(),
    }
}

/// Raw-carrier segment texts of a PROT entry (mirrors [`super::export`]): gated
/// on the dialog-carrier check, skipping anything inside the compressed MAN.
fn raw_seg_texts(entry: &[u8], allow_high: bool) -> Vec<(usize, Vec<u8>)> {
    if !segments::is_dialog_carrier(entry) {
        return Vec::new();
    }
    let compressed = SceneManText::locate(entry).map(|m| m.compressed_span());
    segments::scan_ext(entry, allow_high)
        .iter()
        .filter(|s| !compressed.as_ref().is_some_and(|c| c.contains(&s.text_off)))
        .map(|s| (s.text_off, entry[s.text_off..s.text_off + s.len].to_vec()))
        .collect()
}

/// Parse the PROT entry index out of a `man:<idx>:0x..` / `raw:<idx>:0x..` key.
fn key_entry_index(key: &str) -> Option<usize> {
    let mut it = key.split(':');
    let _kind = it.next()?;
    it.next()?.parse().ok()
}

/// Parse the `0x<va>` of a `scus:str:0x<va>` key.
fn key_scus_va(key: &str) -> Option<u32> {
    let hex = key.strip_prefix("scus:str:0x")?;
    u32::from_str_radix(hex, 16).ok()
}

/// Parse the roster slot of a `scus:party:<n>` key.
fn key_party_slot(key: &str) -> Option<usize> {
    key.strip_prefix("scus:party:")?.parse().ok()
}

/// Lift the text on `source` onto `target`'s coordinate space. Returns a
/// filled working pack + a counts-only report. The pack's language is the
/// build's default ([`SourceBuild::lang`]); a fan translation's caller restamps
/// it (`pack.language`) - nothing in the exe name says what language a patch
/// carries.
pub fn lift_official(
    target: &DiscPatcher,
    source: &DiscPatcher,
) -> Result<(LanguagePack, LiftReport)> {
    let exe_name = boot_exe_name(source)?;
    let Some(build) = source_build_for_exe(&exe_name) else {
        bail!(
            "source boot exe {exe_name:?} is not a Latin-script Legaia build this lift can read \
             (known: SCUS_942.54, SCES_017.52, SCES_019.44/.45/.46/.47; the JP discs use a \
             different text encoding)"
        );
    };
    let lang = build.lang;
    let (pinned_tables, pinned_party) = match build.bases {
        SourceBases::Pinned(p) => (p.table_bases.map(Some), Some(p.party_base)),
        SourceBases::Located => ([None; 4], None),
    };

    // Start from the USA source pack: correct keys, budgets, and `source` text.
    let mut pack = export_pack(target)?;
    pack.language = lang.to_string();
    pack.notes = format!(
        "Text lifted from {exe_name} ({label}) onto USA coordinates (translate \
         lift-official). Contains the game's text - scratchpad only, never commit.",
        label = build.label
    );

    let mut report = LiftReport {
        language: lang.to_string(),
        exe_name: exe_name.clone(),
        build_label: build.label.to_string(),
        ..Default::default()
    };

    // ---- Name tables: build usa_string_va -> pal_string ----
    let usa_exe = target
        .read_named_file("SCUS_942.54")
        .context("SCUS_942.54 not found on target disc")?;
    let pal_exe = source
        .read_named_file(&exe_name)
        .with_context(|| format!("{exe_name} not found on source disc"))?;

    let mut str_map: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for (spec, &pinned) in TABLES.iter().zip(&pinned_tables) {
        // Validate/locate against every pointer field the table owns, then
        // the meta columns.
        let located = locate_base(&usa_exe, &pal_exe, spec, pinned);
        let mut stat = TableStat {
            name: spec.name,
            located: located.is_some(),
            pal_base: located.map(|(b, _)| b).or(pinned).unwrap_or(spec.usa_base),
            valid_fraction: located.map(|(_, f)| f).unwrap_or(0.0),
            paired: 0,
        };
        if let Some((pal_base, _)) = located {
            for id in 0..spec.count {
                for &field in spec.fields {
                    let Some(usa_str_va) =
                        read_ptr(&usa_exe, spec.usa_base + id * spec.stride + field)
                    else {
                        continue;
                    };
                    if usa_str_va == 0 {
                        continue;
                    }
                    let Some(pal_str_va) = read_ptr(&pal_exe, pal_base + id * spec.stride + field)
                    else {
                        continue;
                    };
                    if pal_str_va == 0 {
                        continue;
                    }
                    let Some(pal_str) = read_cstr(&pal_exe, pal_str_va) else {
                        continue;
                    };
                    if str_map.insert(usa_str_va, pal_str).is_none() {
                        stat.paired += 1;
                    }
                }
            }
        }
        report.tables.push(stat);
    }

    // Fill the name-table `scus:str` entries from the map (the `system_text`
    // pools are SCUS strings too, but not pointer-addressed - they pair by
    // pool below).
    let sections = &mut pack.sections;
    for entries in [
        &mut sections.items,
        &mut sections.item_types,
        &mut sections.spells,
        &mut sections.arts,
        &mut sections.accessory_passives,
    ] {
        for e in entries.iter_mut() {
            let Some(va) = key_scus_va(&e.key) else {
                continue;
            };
            match str_map.get(&va) {
                Some(bytes) => {
                    e.translation = markup::decode(bytes);
                    report.names_filled += 1;
                }
                None => report.names_unmapped += 1,
            }
        }
    }

    // ---- Party names: fixed 10-byte fields, id-for-id ----
    report.party_total = pack.sections.party_names.len();
    let party = locate_party_base(&usa_exe, &pal_exe, pinned_party);
    report.party_base = party.map(|(b, _)| b);
    report.party_fingerprint_ok = party.is_some_and(|(_, ok)| ok);
    for e in pack.sections.party_names.iter_mut() {
        let Some((party_base, _)) = party else {
            break;
        };
        let Some(slot) = key_party_slot(&e.key) else {
            continue;
        };
        if slot >= new_game::PARTY_RECORDS {
            continue;
        }
        let va = party_base + (slot * new_game::RECORD_STRIDE) as u32 + 16;
        let Some(off) = item_names::file_offset_for_va(&pal_exe, va) else {
            continue;
        };
        let Some(field) = pal_exe.get(off..off + new_game::NAME_LEN) else {
            continue;
        };
        let len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        if len == 0 {
            continue;
        }
        e.translation = markup::decode(&field[..len]);
        report.party_filled += 1;
    }

    // ---- Dialog: structural pairing per PROT entry ----
    // Group MAN / raw pack entries by PROT index (they are already in scan
    // order within a group, matching the USA scan the pack was built from).
    let man_stats = fill_dialog(
        target,
        source,
        &mut pack.sections.scene_dialog,
        DialogDomain {
            seg_texts: man_seg_texts,
            man_of: man_of_scene,
        },
        &mut report.man_total,
        &mut report.man_paired,
    )?;
    report.man_pairing = man_stats;
    let raw_stats = fill_dialog(
        target,
        source,
        &mut pack.sections.inline_text,
        DialogDomain {
            seg_texts: raw_seg_texts,
            man_of: man_of_stream,
        },
        &mut report.raw_total,
        &mut report.raw_paired,
    )?;
    report.raw_pairing = raw_stats;

    // ---- Overlay UI pools, SCUS system strings, place-name cells ----
    lift_ui_pools(target, source, &mut pack, &mut report);
    let drift = report
        .tables
        .iter()
        .zip(TABLES)
        .filter(|(t, _)| t.located)
        .map(|(t, spec)| t.pal_base as i64 - spec.usa_base as i64)
        .next_back()
        .unwrap_or(0);
    lift_scus_pools(&usa_exe, &pal_exe, drift, &mut pack, &mut report);
    lift_place_cells(&usa_exe, &pal_exe, drift, &mut pack, &mut report);

    // The source-side text is written to fit its own slots, padded with
    // trailing spaces where a translator kept a line same-size; those spaces
    // draw nothing and only cost budget on the target, so they are dropped.
    for entries in pack.sections.each_mut() {
        for e in entries.iter_mut() {
            let trimmed = e.translation.trim_end_matches(' ');
            if !trimmed.is_empty() && trimmed.len() != e.translation.len() {
                e.translation.truncate(trimmed.len());
            }
        }
    }

    Ok((pack, report))
}

/// Every text lead a MAN's clean script walk reaches, in walk order: the
/// structural coordinate of a dialog line. Records are enumerated the way the
/// census does (partition-then-record), and a line's coordinate is `(record
/// ordinal, ordinal among that record's text leads)` - the same on every
/// build, because the script is the same program with different strings.
struct WalkTexts {
    /// The MAN's record shape - a lift only trusts the coordinates when both
    /// discs' MANs have the same one.
    shape: (usize, [i16; 3]),
    /// Text offset (first glyph byte) -> coordinate.
    by_off: BTreeMap<usize, (usize, usize)>,
    /// Coordinate -> text offset.
    by_ord: BTreeMap<(usize, usize), usize>,
}

/// Walk every record of `man` and collect its text leads. `None` when the
/// buffer is not a MAN.
fn walk_texts(man: &[u8]) -> Option<WalkTexts> {
    let mf = man_section::parse(man).ok()?;
    let spans = field_disasm::man_script_spans(&mf, man);
    let mut w = WalkTexts {
        shape: (spans.len(), mf.header.partition_counts),
        by_off: BTreeMap::new(),
        by_ord: BTreeMap::new(),
    };
    for (rec, (_partition, _index, start, pc0, len)) in spans.into_iter().enumerate() {
        let end = start + len;
        let mut pc = start + pc0;
        let mut k = 0usize;
        while pc < end {
            let Ok(insn) = field_disasm::decode(man, pc) else {
                break;
            };
            if insn.size == 0 {
                break;
            }
            if let Some(lead) = field_disasm::text_lead(man, &insn) {
                let off = lead + 1;
                w.by_off.insert(off, (rec, k));
                w.by_ord.insert((rec, k), off);
                k += 1;
            }
            pc += insn.size;
        }
    }
    Some(w)
}

/// The text bytes of the `0x1F`-framed line whose first glyph is at `off`:
/// up to (not including) its `0x00` terminator. `None` for an empty or
/// unterminated run.
fn text_at(man: &[u8], off: usize) -> Option<&[u8]> {
    // Token-walk, not a byte scan: a two-byte token such as `{c1:00}` carries
    // a `0x00` operand that is not the terminator.
    let term = segments::walk_to_terminator(man, off)?;
    (man[term] == 0x00 && term > off && term - off <= MAX_STRLEN).then(|| &man[off..term])
}

/// The MAN a dialog domain's keys index into: `(entry offset of the MAN's
/// byte 0, MAN bytes)`. `man:` keys are decompressed-MAN offsets (base 0);
/// `raw:` keys are entry offsets into a streaming scene whose MAN chunk sits
/// four bytes in.
type ManOf = fn(&[u8]) -> Option<(usize, Vec<u8>)>;

fn man_of_scene(entry: &[u8]) -> Option<(usize, Vec<u8>)> {
    SceneManText::locate(entry).map(|m| (0, m.decoded))
}

fn man_of_stream(entry: &[u8]) -> Option<(usize, Vec<u8>)> {
    StreamManText::locate(entry).map(|m| (m.man_range().start, m.man))
}

/// One dialog domain: how to list its scan-ordered segments (the positional
/// fallback) and how to reach its MAN (the structural pairing).
struct DialogDomain {
    seg_texts: SegTexts,
    man_of: ManOf,
}

/// Blank every `translation` in `pack` that the `baseline` pack also carries -
/// the same text at the same key, or anywhere in the same PROT entry (a patch
/// that adds or removes a line shifts the positional pairing of the rest of
/// that entry, so the same USA key can pair one retail line on the patched
/// disc and its neighbour on the retail one). This is what makes a lift off a
/// **fan-patched** disc distributable: with the retail disc it was built on as
/// the baseline, what survives is the translator's own text, and the lines the
/// patch left alone stay empty (vanilla on import) instead of carrying the
/// underlying build's official text. Returns the number of entries blanked.
pub fn drop_baseline_text(pack: &mut LanguagePack, baseline: &LanguagePack) -> usize {
    use std::collections::{BTreeMap, BTreeSet};
    let group_of = |key: &str| -> String {
        match key_entry_index(key) {
            Some(idx) if key.starts_with("man:") => format!("man:{idx}"),
            Some(idx) if key.starts_with("raw:") => format!("raw:{idx}"),
            _ => key.to_string(),
        }
    };
    let mut blanked = 0usize;
    // Both walks are in serialization order, so the sections pair up.
    for (section, (_, base_entries)) in pack
        .sections
        .each_mut()
        .into_iter()
        .zip(baseline.sections.iter())
    {
        let mut groups: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
        for e in base_entries {
            if !e.translation.is_empty() {
                groups
                    .entry(group_of(&e.key))
                    .or_default()
                    .insert(e.translation.as_str());
            }
        }
        for e in section.iter_mut() {
            if e.translation.is_empty() {
                continue;
            }
            let shared = groups
                .get(&group_of(&e.key))
                .is_some_and(|set| set.contains(e.translation.as_str()));
            if shared {
                e.translation.clear();
                blanked += 1;
            }
        }
    }
    blanked
}

/// ASCII-fold every lifted `translation` in `pack`, in place.
///
/// The official PAL text uses accented glyph cells the NTSC font leaves empty,
/// so a lifted pack imported as-is renders blanks where the accents were unless
/// the font atlas is patched too (a separate deliverable - see
/// `docs/tooling/pal-localizations.md`). Folding trades the accents for text
/// that renders correctly on an unmodified USA disc: `{82}` (e-acute) becomes
/// `e`, and so on across the CP437-aligned accent block.
///
/// Returns counts only. `source` fields are untouched (they are USA text and
/// already plain ASCII).
pub fn fold_pack_accents(pack: &mut LanguagePack) -> markup::FoldStats {
    let mut stats = markup::FoldStats::default();
    for entries in pack.sections.each_mut() {
        for e in entries.iter_mut() {
            if e.translation.is_empty() {
                continue;
            }
            let (folded, s) = markup::fold_high_glyphs(&e.translation);
            e.translation = folded;
            stats.merge(s);
        }
    }
    stats
}

/// Extractor of a PROT entry's ordered segment texts (offset + bytes) in one
/// dialog domain (MAN or raw), given the PAL-tolerant `allow_high` flag.
type SegTexts = fn(&[u8], bool) -> Vec<(usize, Vec<u8>)>;

/// Fill a dialog section's translations. A line is paired **structurally**
/// first - the same `(record, ordinal)` on both discs' clean script walks
/// ([`walk_texts`]) - and by scan ordinal only where the walk does not place
/// it (an operand run, text past a record's first decode error, or an entry
/// whose MAN has a different record shape on the source disc).
///
/// The structural coordinate is what makes the pairing robust to the source
/// disc adding, removing or re-shaping a line: a fan patch that rewrites
/// `Yes` as `S.` drops that line out of the scan's quality gate, and every
/// scan ordinal after it in the scene then names the *previous* line - a
/// shift the counts never show. The walk does not care what the text is.
fn fill_dialog(
    target: &DiscPatcher,
    source: &DiscPatcher,
    entries: &mut [super::pack::Entry],
    domain: DialogDomain,
    total: &mut usize,
    paired: &mut usize,
) -> Result<PairStats> {
    use std::collections::BTreeMap;
    let mut stats = PairStats::default();
    // Group entry-vec indices by PROT index.
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, e) in entries.iter().enumerate() {
        *total += 1;
        if let Some(idx) = key_entry_index(&e.key) {
            groups.entry(idx).or_default().push(i);
        }
    }
    for (prot, members) in groups {
        let (Ok(usa_entry), Ok(pal_entry)) = (target.read_entry(prot), source.read_entry(prot))
        else {
            continue;
        };
        // Both sides are scanned with the accent-tolerant gate. The pack was
        // exported with the strict gate, but every strict segment is also a
        // tolerant one at the same offset (the gate only relaxes the glyph
        // test, and a qualifying run never contains a `0x1F` start), so the
        // pack's offsets index the tolerant list - and only the tolerant list
        // sees the *same* coincidental high-byte hits on both discs. Scanning
        // the USA side strict skipped those hits on one side only and shifted
        // every ordinal after the first one in the entry: a retail disc lifted
        // onto itself came back with other lines' text.
        let usa_list = (domain.seg_texts)(&usa_entry, true);
        let pal_list = (domain.seg_texts)(&pal_entry, true);
        // Map USA text offset -> ordinal.
        let ord_of: BTreeMap<usize, usize> = usa_list
            .iter()
            .enumerate()
            .map(|(k, (off, _))| (*off, k))
            .collect();
        // The structural coordinates, when both MANs are the same program.
        let walks = match ((domain.man_of)(&usa_entry), (domain.man_of)(&pal_entry)) {
            (Some((ub, um)), Some((pb, pm))) => match (walk_texts(&um), walk_texts(&pm)) {
                (Some(uw), Some(pw)) if uw.shape == pw.shape => Some((ub, uw, pb, pm, pw)),
                _ => None,
            },
            _ => None,
        };
        for &m in &members {
            let key = entries[m].key.clone();
            // Offset from the key.
            let Some(off) = key
                .rsplit(":0x")
                .next()
                .and_then(|h| usize::from_str_radix(h, 16).ok())
            else {
                continue;
            };
            let positional = ord_of
                .get(&off)
                .and_then(|&k| pal_list.get(k))
                .map(|(o, t)| (*o, t.as_slice()));
            let structural = walks.as_ref().and_then(|(ub, uw, pb, pm, pw)| {
                let man_off = off.checked_sub(*ub)?;
                let coord = uw.by_off.get(&man_off)?;
                let pal_off = *pw.by_ord.get(coord)?;
                text_at(pm, pal_off).map(|t| (pal_off + pb, t))
            });
            let chosen = match (structural, positional) {
                (Some((pal_off, text)), pos) => {
                    stats.structural += 1;
                    if pos.is_none_or(|(o, _)| o != pal_off) {
                        stats.shifted += 1;
                    }
                    Some(text)
                }
                (None, Some((_, text))) => {
                    stats.positional += 1;
                    Some(text)
                }
                (None, None) => None,
            };
            if let Some(text) = chosen {
                entries[m].translation = markup::decode(text);
                *paired += 1;
            }
        }
    }
    Ok(stats)
}

/// NUL-to-NUL chunks of `buf[lo..hi)`: `(offset, bytes)` of every non-empty
/// run, junk included - the raw material the pool pairing anchors on.
fn nul_chunks(buf: &[u8], lo: usize, hi: usize) -> Vec<(usize, Vec<u8>)> {
    let hi = hi.min(buf.len());
    let mut out = Vec::new();
    let mut pos = lo;
    while pos < hi {
        if buf[pos] == 0 {
            pos += 1;
            continue;
        }
        let mut e = pos;
        while e < hi && buf[e] != 0 {
            e += 1;
        }
        // A string pool's first string can trail the code that addresses it
        // with no NUL between: key the chunk on its first glyph, not on the
        // control bytes ahead of it, so the two builds' chunks compare.
        let start = pos + buf[pos..e].iter().take_while(|&&b| b < 0x20).count();
        // A run that is control bytes only is kept whole: the build-invariant
        // byte runs between two strings are what anchor the pairing.
        let start = if start < e { start } else { pos };
        out.push((start, buf[start..e].to_vec()));
        pos = e;
    }
    out
}

/// Pair two NUL-chunk lists of the *same* string pool on two builds:
/// `usa offset -> source bytes`.
///
/// Identical chunks that occur once on each side (a stat label, a proper
/// noun, a run of pointer bytes the two builds share) are **anchors**; they
/// partition both lists, and inside a partition the chunks pair by ordinal
/// when the two sides count the same. Where they do not - the source build
/// carries an extra string, or a window edge cut a run - the partition
/// retries on the prose-shaped chunks only (`segments::qualifies_ext`), and
/// an edge partition aligns on its anchor side. Nothing pairs across a
/// disagreement: an unpaired label stays vanilla on import, which is the
/// safe failure.
fn pair_chunk_lists(
    usa: &[(usize, Vec<u8>)],
    src: &[(usize, Vec<u8>)],
) -> BTreeMap<usize, Vec<u8>> {
    use std::collections::HashMap;
    fn count(list: &[(usize, Vec<u8>)]) -> HashMap<&[u8], usize> {
        let mut m: HashMap<&[u8], usize> = HashMap::new();
        for (_, b) in list {
            *m.entry(b.as_slice()).or_default() += 1;
        }
        m
    }
    let (cu, cs) = (count(usa), count(src));
    let src_index: HashMap<&[u8], usize> = src
        .iter()
        .enumerate()
        .map(|(i, (_, b))| (b.as_slice(), i))
        .collect();
    // Anchors, monotonic in both lists.
    let mut anchors: Vec<(usize, usize)> = Vec::new();
    let mut last = 0usize;
    for (i, (_, b)) in usa.iter().enumerate() {
        if cu.get(b.as_slice()) == Some(&1)
            && cs.get(b.as_slice()) == Some(&1)
            && let Some(&j) = src_index.get(b.as_slice())
            && (anchors.is_empty() || j > last)
        {
            anchors.push((i, j));
            last = j;
        }
    }
    let mut out = BTreeMap::new();
    let mut bounds = vec![(0usize, 0usize)];
    bounds.extend(anchors.iter().map(|&(i, j)| (i + 1, j + 1)));
    let ends: Vec<(usize, usize)> = anchors
        .iter()
        .map(|&(i, j)| (i, j))
        .chain(std::iter::once((usa.len(), src.len())))
        .collect();
    for (p, (&(ui, sj), &(ue, se))) in bounds.iter().zip(&ends).enumerate() {
        let (u, s) = (&usa[ui..ue], &src[sj..se]);
        if u.len() == s.len() {
            for (a, b) in u.iter().zip(s) {
                out.insert(a.0, b.1.clone());
            }
            continue;
        }
        // The source window is widened past the pool on purpose (its strings
        // run longer), so a partition's surplus is run-in before or run-out
        // after the pool. A partition anchored on one side aligns on that
        // side; a window with no anchor tries the head first. Either way the
        // aligned pairs must agree pairwise in shape (prose against prose,
        // pointer bytes against pointer bytes, comparable length), and the
        // same is retried over the prose-shaped chunks only, so a run of
        // one-byte junk on one side cannot displace a line.
        let head_first = p + 1 == bounds.len();
        let prose = |c: &(usize, Vec<u8>)| segments::qualifies_ext(&c.1, true);
        let up: Vec<&(usize, Vec<u8>)> = u.iter().filter(|c| prose(c)).collect();
        let sp: Vec<&(usize, Vec<u8>)> = s.iter().filter(|c| prose(c)).collect();
        let u_all: Vec<&(usize, Vec<u8>)> = u.iter().collect();
        let s_all: Vec<&(usize, Vec<u8>)> = s.iter().collect();
        let pairs =
            align_chunks(&u_all, &s_all, head_first).or_else(|| align_chunks(&up, &sp, head_first));
        if let Some(pairs) = pairs {
            for (a, b) in pairs {
                out.insert(a.0, b.1.clone());
            }
        }
    }
    for &(i, j) in &anchors {
        out.insert(usa[i].0, src[j].1.clone());
    }
    out
}

/// Two chunks of the same pool slot on two builds look alike: both prose or
/// both not, and when prose, within a length ratio a translation stays inside.
fn chunks_agree(a: &[u8], b: &[u8]) -> bool {
    let (pa, pb) = (
        segments::qualifies_ext(a, true),
        segments::qualifies_ext(b, true),
    );
    if pa != pb {
        return false;
    }
    if !pa {
        return true;
    }
    let (la, lb) = (a.len().max(1), b.len().max(1));
    la * 2 <= lb * 5 && lb * 2 <= la * 5
}

/// A NUL-delimited chunk of a string pool: `(offset, bytes)`.
type Chunk = (usize, Vec<u8>);

/// Align two chunk lists of unequal length by their head or their tail,
/// whichever the caller prefers, accepting an alignment only when every
/// pair [`chunks_agree`]s. `None` when neither does.
fn align_chunks<'a>(
    u: &[&'a Chunk],
    s: &[&'a Chunk],
    head_first: bool,
) -> Option<Vec<(&'a Chunk, &'a Chunk)>> {
    let n = u.len().min(s.len());
    if n == 0 {
        return None;
    }
    let head: Vec<_> = u[..n].iter().zip(&s[..n]).map(|(a, b)| (*a, *b)).collect();
    let tail: Vec<_> = u[u.len() - n..]
        .iter()
        .zip(&s[s.len() - n..])
        .map(|(a, b)| (*a, *b))
        .collect();
    let order = if head_first {
        [head, tail]
    } else {
        [tail, head]
    };
    order
        .into_iter()
        .find(|pairs| pairs.iter().all(|(a, b)| chunks_agree(&a.1, &b.1)))
}

/// Fill the `ui:` entries: each pinned overlay pool is read on both discs,
/// the pool window on the source side widened (its strings are longer or
/// shorter, the pool moves), and the two NUL-chunk lists paired by anchors
/// ([`pair_chunk_lists`]).
fn lift_ui_pools(
    target: &DiscPatcher,
    source: &DiscPatcher,
    pack: &mut LanguagePack,
    report: &mut LiftReport,
) {
    for pool in ui::UI_STRING_POOLS {
        let (Ok(usa_entry), Ok(src_entry)) = (
            target.read_entry(pool.prot_index),
            source.read_entry(pool.prot_index),
        ) else {
            continue;
        };
        let lo = (pool.va_start - pool.base_va) as usize;
        let hi = (pool.va_end - pool.base_va) as usize;
        let span = hi - lo;
        let usa_chunks = nul_chunks(&usa_entry, lo.saturating_sub(0x40), hi + 0x40);
        let src_chunks = nul_chunks(&src_entry, lo.saturating_sub(0x40), hi + span + 0x200);
        let map = pair_chunk_lists(&usa_chunks, &src_chunks);
        if std::env::var("LEGAIA_LIFT_DEBUG_POOL").ok().as_deref()
            == Some(&pool.prot_index.to_string())
        {
            for (o, b) in &usa_chunks {
                eprintln!(
                    "U 0x{o:05x} {:?} -> {:?}",
                    String::from_utf8_lossy(b),
                    map.get(o).map(|m| String::from_utf8_lossy(m).into_owned())
                );
            }
            for (o, b) in &src_chunks {
                eprintln!("S 0x{o:05x} {:?}", String::from_utf8_lossy(b));
            }
        }
        let mut stat = PoolStat {
            label: format!("{} (PROT {})", pool.label, pool.prot_index),
            paired: 0,
            total: 0,
        };
        let prefix = format!("ui:{}:0x", pool.prot_index);
        for e in pack.sections.ui_menu.iter_mut() {
            let Some(hex) = e.key.strip_prefix(&prefix) else {
                continue;
            };
            let Ok(va) = u32::from_str_radix(hex, 16) else {
                continue;
            };
            if va < pool.va_start || va >= pool.va_end {
                continue;
            }
            stat.total += 1;
            let off = (va - pool.base_va) as usize;
            // A pool's first string can trail the code that addresses it with
            // no NUL between (the tutorial pool opens six code bytes ahead of
            // its first line): the chunk pairs as a whole, and the string is
            // its tail past the shared prefix.
            let hit = map.get(&off).cloned().or_else(|| {
                let (start, _) = usa_chunks
                    .iter()
                    .find(|(start, chunk)| *start < off && off < start + chunk.len())?;
                let prefix = off - start;
                map.get(start)
                    .filter(|src| src.len() > prefix)
                    .map(|src| src[prefix..].to_vec())
            });
            if let Some(bytes) = hit {
                e.translation = markup::decode(&bytes);
                stat.paired += 1;
            }
        }
        report.ui_paired += stat.paired;
        report.ui_total += stat.total;
        report.ui_pools.push(stat);
    }
}

/// Fill the `system_text` entries: the SCUS pools have no located base of
/// their own, so the source window is the USA window carried by the data
/// segment's `drift` (the located name tables' displacement) with a margin,
/// and the chunks pair as for an overlay pool.
fn lift_scus_pools(
    usa_exe: &[u8],
    src_exe: &[u8],
    drift: i64,
    pack: &mut LanguagePack,
    report: &mut LiftReport,
) {
    for pool in ui::SCUS_STRING_POOLS {
        let lo = (pool.va_start - pool.base_va) as usize;
        let hi = (pool.va_end - pool.base_va) as usize;
        let span = hi - lo;
        // Both windows carry the same run-in and run-out (the debug labels
        // and pointer bytes around the pool are build-invariant), which is
        // what anchors the pairing where the pool's own strings all differ.
        let usa_chunks = nul_chunks(usa_exe, lo.saturating_sub(0x40), hi + 0x80);
        let s_lo = (lo as i64 + drift - 0x40).max(0) as usize;
        let s_hi = (hi as i64 + drift + span as i64 + 0x80).max(0) as usize;
        let src_chunks = nul_chunks(src_exe, s_lo, s_hi);
        let map = pair_chunk_lists(&usa_chunks, &src_chunks);
        if std::env::var("LEGAIA_LIFT_DEBUG_POOL").ok().as_deref() == Some("scus") {
            for (o, b) in &usa_chunks {
                eprintln!(
                    "U 0x{o:05x} {:?} -> {:?}",
                    String::from_utf8_lossy(b),
                    map.get(o).map(|m| String::from_utf8_lossy(m).into_owned())
                );
            }
            for (o, b) in &src_chunks {
                eprintln!("S 0x{o:05x} {:?}", String::from_utf8_lossy(b));
            }
        }
        for e in pack.sections.system_text.iter_mut() {
            let Some(va) = key_scus_va(&e.key) else {
                continue;
            };
            if va < pool.va_start || va >= pool.va_end {
                continue;
            }
            report.system_total += 1;
            let off = (va - pool.base_va) as usize;
            if let Some(bytes) = map.get(&off) {
                e.translation = markup::decode(bytes);
                report.system_paired += 1;
            }
        }
    }
}

/// Fill the `place_names` cells: the source exe's cell table is found by
/// its first cell (the home town's name, a proper noun every build shares)
/// near the drifted USA address, and the cells pair id-for-id.
fn lift_place_cells(
    usa_exe: &[u8],
    src_exe: &[u8],
    drift: i64,
    pack: &mut LanguagePack,
    report: &mut LiftReport,
) {
    const STRIDE: usize = worldmap_menu::NAME_STRIDE;
    let usa_va = worldmap_menu::NAME_TABLE_ADDR;
    let Some(usa_off) = item_names::file_offset_for_va(usa_exe, usa_va) else {
        return;
    };
    let Some(first) = usa_exe.get(usa_off..usa_off + STRIDE) else {
        return;
    };
    let centre = (usa_off as i64 + drift).max(0) as usize;
    let lo = centre.saturating_sub(0x1000);
    let hi = (centre + 0x1000).min(src_exe.len().saturating_sub(STRIDE));
    let hit = (lo..=hi)
        .filter(|&o| o.is_multiple_of(4) && &src_exe[o..o + STRIDE] == first)
        .min_by_key(|&o| o.abs_diff(centre));
    let Some(src_off) = hit else {
        report.cells_total += pack.sections.place_names.len();
        return;
    };
    for e in pack.sections.place_names.iter_mut() {
        let Some(hex) = e.key.strip_prefix("scus:cell:0x") else {
            continue;
        };
        let Ok(va) = u32::from_str_radix(hex, 16) else {
            continue;
        };
        report.cells_total += 1;
        let Some(n) = va
            .checked_sub(usa_va)
            .map(|d| d as usize / STRIDE)
            .filter(|&n| n < worldmap_menu::NAME_COUNT)
        else {
            continue;
        };
        let Some(cell) = src_exe.get(src_off + n * STRIDE..src_off + (n + 1) * STRIDE) else {
            continue;
        };
        let len = cell.iter().position(|&b| b == 0).unwrap_or(cell.len());
        if len == 0 || cell[..len].iter().any(|&b| b < 0x20) {
            continue;
        }
        e.translation = markup::decode(&cell[..len]);
        report.cells_paired += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_filter_blanks_shared_text_per_entry() {
        use super::super::pack::Entry;
        let entry = |key: &str, t: &str| Entry {
            key: key.to_string(),
            context: String::new(),
            source: String::new(),
            translation: t.to_string(),
            budget: 8,
        };
        let mut pack = LanguagePack::new("xx");
        pack.sections.scene_dialog = vec![
            entry("man:5:0x10", "same key"), // blanked: baseline has it at this key
            entry("man:5:0x20", "shifted line"), // blanked: baseline has it elsewhere in entry 5
            entry("man:5:0x30", "translated"), // kept
            entry("man:6:0x10", "shifted line"), // kept: entry 6 never carried it
        ];
        pack.sections.items = vec![entry("scus:str:0x80011230", "Potion")];
        let mut base = LanguagePack::new("xx");
        base.sections.scene_dialog = vec![
            entry("man:5:0x10", "same key"),
            entry("man:5:0x28", "shifted line"),
            entry("man:5:0x30", "retail text"),
        ];
        base.sections.items = vec![entry("scus:str:0x80011230", "Potion")];
        assert_eq!(drop_baseline_text(&mut pack, &base), 3);
        let t: Vec<&str> = pack
            .sections
            .scene_dialog
            .iter()
            .map(|e| e.translation.as_str())
            .collect();
        assert_eq!(t, ["", "", "translated", "shifted line"]);
        assert_eq!(pack.sections.items[0].translation, "");
    }

    #[test]
    fn source_build_map_covers_every_latin_build() {
        let lang = |exe| source_build_for_exe(exe).map(|b| b.lang);
        assert_eq!(lang("SCES_019.44"), Some("fr"));
        assert_eq!(lang("SCES_019.45"), Some("de"));
        assert_eq!(lang("SCES_019.46"), Some("it"));
        assert_eq!(lang("SCES_019.47"), Some("es"));
        assert_eq!(lang("SCES_017.52"), Some("en"));
        assert_eq!(lang("SCUS_942.54"), Some("en"));
        // The measured three are pinned; the rest are located from the USA VAs.
        for (exe, pinned) in [
            ("SCES_019.44", true),
            ("SCES_019.46", true),
            ("SCES_019.47", false),
            ("SCUS_942.54", false),
        ] {
            let b = source_build_for_exe(exe).unwrap();
            assert_eq!(matches!(b.bases, SourceBases::Pinned(_)), pinned, "{exe}");
        }
        // Not Latin-script: the JP original and the demos.
        assert!(source_build_for_exe("SCPS_100.59").is_none());
        assert!(source_build_for_exe("SCUS_943.66").is_none());
    }

    #[test]
    fn key_parsers() {
        assert_eq!(key_scus_va("scus:str:0x80011230"), Some(0x8001_1230));
        assert_eq!(key_scus_va("man:31:0xe7"), None);
        assert_eq!(key_party_slot("scus:party:2"), Some(2));
        assert_eq!(key_entry_index("man:874:0x1a"), Some(874));
        assert_eq!(key_entry_index("raw:12:0x0"), Some(12));
    }

    #[test]
    fn name_shape_gate() {
        assert!(looks_like_name(b"Gl\x81cksglocke")); // German u-umlaut byte
        assert!(looks_like_name(&[0x01, b'K', b'e', b'y'])); // icon prefix
        assert!(!looks_like_name(b"")); // empty
        assert!(!looks_like_name(&[0x00, 0x03, 0x1f])); // control bytes / code
    }
}
