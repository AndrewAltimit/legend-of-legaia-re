//! Where the source build keeps its name tables: the per-table specs, the
//! pinned PAL bases, and the pointer-walk locator for an unpinned executable.

use super::super::markup;
use legaia_game_tables::{item_names, new_game};

/// Longest string the pooled-name reader follows before calling a pointer bogus.
pub(super) const MAX_STRLEN: usize = 512;

/// One SCUS name table: a pointer table with a shared record stride, plus the
/// per-record byte offsets of the string-pointer fields it owns. The USA base
/// is the pack's coordinate space; the PAL base is located per disc.
pub(super) struct TableSpec {
    /// Reporting label.
    pub(super) name: &'static str,
    /// USA virtual address of record 0's first pointed field.
    pub(super) usa_base: u32,
    /// Record stride in bytes.
    pub(super) stride: u32,
    /// Number of records to walk.
    pub(super) count: u32,
    /// Pointer-field byte offsets within a record (relative to `usa_base`)
    /// whose strings the lift carries.
    pub(super) fields: &'static [u32],
    /// Every pointer word in the record, lifted or not. The bytes outside
    /// these words are the record's **meta columns** (stats, ids, scope), the
    /// same on every build, and they fingerprint the base: a same-shaped
    /// alias (any item-table record read four bytes in has the accessory
    /// record's `[meta, ptr, ptr]` shape) validates as names but not as meta.
    ptr_words: &'static [u32],
}

/// The five pooled-string tables (party names are a fixed field, handled apart).
/// Bases mirror the parser constants (`item_names::TABLE_VA` etc.); fields are
/// the exact pointer offsets [`super::super::export`] collects.
pub(super) const TABLES: &[TableSpec] = &[
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
        usa_base: 0x8007_5EC4, // arts_table::TABLE_VA; +0xC = name, +0x10 = description
        stride: 0x14,
        count: 256,
        fields: &[0xC, 0x10],
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
pub(super) struct PalBases {
    /// Pinned base of each [`TABLES`] entry, same order.
    pub(super) table_bases: [u32; 4],
    /// Pinned new-game party-template base.
    pub(super) party_base: u32,
}

/// Where a source build's table bases come from.
#[derive(Debug, Clone, Copy)]
pub(super) enum SourceBases {
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
    pub(super) bases: SourceBases,
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
                table_bases: [
                    item_names::PAL_TABLE_VAS[0].1,
                    0x8007_5DA8,
                    0x8007_67A4,
                    0x8007_6B3C,
                ],
                party_base: 0x8007_9508,
            }),
        )),
        "SCES_019.45" => Some(build(
            "de",
            "Germany (PAL)",
            SourceBases::Pinned(PalBases {
                table_bases: [
                    item_names::PAL_TABLE_VAS[1].1,
                    0x8007_64BC,
                    0x8007_6EB8,
                    0x8007_7250,
                ],
                party_base: 0x8007_9C78,
            }),
        )),
        "SCES_019.46" => Some(build(
            "it",
            "Italy (PAL)",
            SourceBases::Pinned(PalBases {
                table_bases: [
                    item_names::PAL_TABLE_VAS[2].1,
                    0x8007_628C,
                    0x8007_6C88,
                    0x8007_7020,
                ],
                party_base: 0x8007_9A14,
            }),
        )),
        "SCES_019.47" => Some(build("es", "Spain (PAL)", SourceBases::Located)),
        _ => None,
    }
}

/// Read the little-endian pointer word at `va` in a PS-X EXE image.
pub(super) fn read_ptr(exe: &[u8], va: u32) -> Option<u32> {
    let off = item_names::file_offset_for_va(exe, va)?;
    Some(u32::from_le_bytes(exe.get(off..off + 4)?.try_into().ok()?))
}

/// Read the NUL-terminated string at `va` (raw bytes, no terminator).
pub(super) fn read_cstr(exe: &[u8], va: u32) -> Option<Vec<u8>> {
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
pub(super) fn looks_like_name(bytes: &[u8]) -> bool {
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
pub(super) fn locate_base(
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
pub(super) fn locate_party_base(
    usa_exe: &[u8],
    src_exe: &[u8],
    pinned: Option<u32>,
) -> Option<(u32, bool)> {
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
