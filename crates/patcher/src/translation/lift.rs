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
//! - **Dialog** (`man:` scene-bundle MANs, `raw:` event-script carriers):
//!   positional. The Nth qualifying segment of PROT entry `i` on USA pairs with
//!   the Nth on the PAL disc (byte offsets differ - the localized MAN repacks -
//!   but line *order* is the script's, not the text's).
//!
//! The result is a **working pack** (`source:` = USA text, `translation:` =
//! official PAL text) carrying the USA per-string byte budgets. It is filled
//! with the game's copyrighted text, so it is scratchpad-only output - never
//! committed. The lifted `translation` bytes are the raw PAL bytes decoded to
//! markup (accents become `{82}`-style single-byte escapes), which the
//! [markup codec](super::markup) round-trips exactly.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

use legaia_asset::{item_names, new_game};

use crate::disc::DiscPatcher;

use super::export::{SceneManText, export_pack};
use super::pack::LanguagePack;
use super::{markup, segments};

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

    // Fill scus:str entries from the map.
    for entries in pack.sections.each_mut() {
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

    // ---- Dialog: positional pairing per PROT entry ----
    // Group MAN / raw pack entries by PROT index (they are already in scan
    // order within a group, matching the USA scan the pack was built from).
    fill_dialog(
        target,
        source,
        &mut pack.sections.scene_dialog,
        man_seg_texts,
        &mut report.man_total,
        &mut report.man_paired,
    )?;
    fill_dialog(
        target,
        source,
        &mut pack.sections.inline_text,
        raw_seg_texts,
        &mut report.raw_total,
        &mut report.raw_paired,
    )?;

    Ok((pack, report))
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

/// Fill a dialog section's translations by positional pairing. `seg_texts`
/// extracts the ordered segment texts of a PROT entry in the section's domain.
fn fill_dialog(
    target: &DiscPatcher,
    source: &DiscPatcher,
    entries: &mut [super::pack::Entry],
    seg_texts: SegTexts,
    total: &mut usize,
    paired: &mut usize,
) -> Result<()> {
    use std::collections::BTreeMap;
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
        let usa_list = seg_texts(&usa_entry, true);
        let pal_list = seg_texts(&pal_entry, true);
        // Map USA text offset -> ordinal.
        let ord_of: BTreeMap<usize, usize> = usa_list
            .iter()
            .enumerate()
            .map(|(k, (off, _))| (*off, k))
            .collect();
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
            let Some(&k) = ord_of.get(&off) else {
                continue;
            };
            if let Some((_, text)) = pal_list.get(k) {
                entries[m].translation = markup::decode(text);
                *paired += 1;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
