//! Lift an official PAL localization into a **USA-keyed working pack**.
//!
//! The three official PAL discs (`SCES_019.44`/`.45`/`.46` = FR/DE/IT) are 1:1
//! with the USA disc at the container level (see
//! `docs/tooling/pal-localizations.md`): a USA PROT coordinate names the same
//! logical asset on every disc, the five SCUS name tables exist id-for-id at
//! language-shifted VAs, and the `0x1F`-segment dialog corpus pairs by position
//! within each PROT entry. This module re-keys the official localized text onto
//! the USA coordinate space the [importer](super::import) patches:
//!
//! - **Name tables** (item / spell / arts / accessory / party): id-for-id. The
//!   USA pack keys each pooled string by its *USA* virtual address; the same id
//!   on the PAL exe points at the localized string, so the map is
//!   `usa_string_va -> pal_string`. The PAL base is *located* (verified against
//!   the pinned VA by following its pointers, with a windowed search fallback),
//!   never trusted blind.
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
    /// Pointer-field byte offsets within a record (relative to `usa_base`).
    fields: &'static [u32],
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
    },
    TableSpec {
        name: "spells",
        usa_base: 0x8007_54C8, // spell_names::STATS_VA; +8 = name ptr
        stride: 0x0C,
        count: 256,
        fields: &[8],
    },
    TableSpec {
        name: "arts",
        usa_base: 0x8007_5EC4, // arts_table::TABLE_VA; +0xC = name ptr
        stride: 0x14,
        count: 256,
        fields: &[0xC],
    },
    TableSpec {
        name: "accessory_passives",
        usa_base: 0x8007_625C, // accessory_passive::PASSIVE_TABLE_VA; +4 name, +8 desc
        stride: 0x0C,
        count: 0x40,
        fields: &[4, 8],
    },
];

/// Per-language PAL base VAs for [`TABLES`], in table order, plus the new-game
/// party-template base. Located, not shift-computed - the pointer-table region
/// drifts locally per language (see `docs/tooling/pal-localizations.md`).
struct PalBases {
    /// Pinned base of each [`TABLES`] entry, same order.
    table_bases: [u32; 4],
    /// Pinned new-game party-template base.
    party_base: u32,
}

/// Boot exe name -> `(language code, pinned PAL bases)`. `None` for the USA exe
/// (there is nothing to lift from an NTSC-against-NTSC pairing).
fn region_for_exe(exe: &str) -> Option<(&'static str, PalBases)> {
    match exe {
        "SCES_019.44" => Some((
            "fr",
            PalBases {
                table_bases: [0x8007_4C4C, 0x8007_5DA8, 0x8007_67A4, 0x8007_6B3C],
                party_base: 0x8007_9508,
            },
        )),
        "SCES_019.45" => Some((
            "de",
            PalBases {
                table_bases: [0x8007_5360, 0x8007_64BC, 0x8007_6EB8, 0x8007_7250],
                party_base: 0x8007_9C78,
            },
        )),
        "SCES_019.46" => Some((
            "it",
            PalBases {
                table_bases: [0x8007_5130, 0x8007_628C, 0x8007_6C88, 0x8007_7020],
                party_base: 0x8007_9A14,
            },
        )),
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
    field: u32,
) -> (f64, usize) {
    let mut ok = 0usize;
    let mut seen = 0usize;
    for id in 0..count {
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
    let f = if seen == 0 {
        0.0
    } else {
        ok as f64 / seen as f64
    };
    (f, seen)
}

/// Locate a table's PAL base: accept the pinned VA if it validates against the
/// USA-populated id set, else search a record-aligned window for the offset
/// with the highest valid fraction. Returns `(base, valid_fraction)` or `None`
/// when nothing clears the threshold with a meaningful sample.
fn locate_base(
    usa_exe: &[u8],
    pal_exe: &[u8],
    usa_base: u32,
    pinned: u32,
    stride: u32,
    count: u32,
    field: u32,
) -> Option<(u32, f64)> {
    const THRESHOLD: f64 = 0.75;
    const MIN_SAMPLE: usize = 6;
    let check =
        |cand: u32| base_valid_fraction(usa_exe, pal_exe, usa_base, cand, stride, count, field);
    let (pinned_frac, pinned_n) = check(pinned);
    if pinned_frac >= THRESHOLD && pinned_n >= MIN_SAMPLE {
        return Some((pinned, pinned_frac));
    }
    // Fall back to a windowed search (4-byte-aligned steps).
    let mut best = (pinned, pinned_frac, pinned_n);
    let window = 0x2000i64;
    let mut d = -window;
    while d <= window {
        let cand = (pinned as i64 + d) as u32;
        let (f, n) = check(cand);
        if f > best.1 && n >= MIN_SAMPLE {
            best = (cand, f, n);
        }
        d += 4;
    }
    (best.1 >= THRESHOLD && best.2 >= MIN_SAMPLE).then_some((best.0, best.1))
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
    pub tables: Vec<TableStat>,
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

/// Lift the official localization on `source` onto `target`'s coordinate space.
/// Returns a filled working pack + a counts-only report.
pub fn lift_official(
    target: &DiscPatcher,
    source: &DiscPatcher,
) -> Result<(LanguagePack, LiftReport)> {
    let exe_name = boot_exe_name(source)?;
    let Some((lang, pal)) = region_for_exe(&exe_name) else {
        bail!(
            "source boot exe {exe_name:?} is not a known PAL localization \
             (expected SCES_019.44/.45/.46)"
        );
    };

    // Start from the USA source pack: correct keys, budgets, and `source` text.
    let mut pack = export_pack(target)?;
    pack.language = lang.to_string();
    pack.notes = format!(
        "Official {lang} localization lifted from {exe_name} onto USA coordinates \
         (translate lift-official). Contains the game's text - scratchpad only, never commit."
    );

    let mut report = LiftReport {
        language: lang.to_string(),
        exe_name: exe_name.clone(),
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
    for (spec, &pinned) in TABLES.iter().zip(&pal.table_bases) {
        // Validate/locate against field 0 (the primary name pointer).
        let located = locate_base(
            &usa_exe,
            &pal_exe,
            spec.usa_base,
            pinned,
            spec.stride,
            spec.count,
            spec.fields[0],
        );
        let mut stat = TableStat {
            name: spec.name,
            located: located.is_some(),
            pal_base: located.map(|(b, _)| b).unwrap_or(pinned),
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
    for e in pack.sections.party_names.iter_mut() {
        let Some(slot) = key_party_slot(&e.key) else {
            continue;
        };
        if slot >= new_game::PARTY_RECORDS {
            continue;
        }
        let va = pal.party_base + (slot * new_game::RECORD_STRIDE) as u32 + 16;
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
        // USA side scanned exactly as export did (Latin build: high-gate is a
        // no-op), so `usa_list[k]` is the pack entry at ordinal `k`.
        let usa_list = seg_texts(&usa_entry, false);
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
    fn region_map_covers_the_three_pal_exes() {
        assert_eq!(region_for_exe("SCES_019.44").unwrap().0, "fr");
        assert_eq!(region_for_exe("SCES_019.45").unwrap().0, "de");
        assert_eq!(region_for_exe("SCES_019.46").unwrap().0, "it");
        assert!(region_for_exe("SCUS_942.54").is_none());
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
