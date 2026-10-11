//! The dialog domain of the lift: walking both discs' scene and stream MANs in
//! step, pairing their segments, and filling the pack's dialog section.

use super::super::export::SceneManText;
use super::super::pack::LanguagePack;
use super::super::stream_man::StreamManText;
use super::super::{markup, segments};
use super::chunk_align::framings;
use super::source_tables::MAX_STRLEN;
use super::{PairStats, key_entry_index};
use anyhow::Result;
use legaia_disc_patch::disc::DiscPatcher;
use legaia_scene_formats::field_disasm;
use legaia_scene_formats::man_section;
use std::collections::BTreeMap;

/// Every text lead a MAN's clean script walk reaches, in walk order: the
/// structural coordinate of a dialog line. Records are enumerated the way the
/// census does (partition-then-record), and a line's coordinate is `(record
/// ordinal, ordinal among that record's text leads)` - the same on every
/// build, because the script is the same program with different strings.
///
/// The pages of a narration **crawl** block (`CC F8 80 N` and the `N`
/// `0x1F`-framed pages after it - the opening's creation myth) are the one
/// place a localization changes the program: the page count `N` is part of
/// the script, and the PAL builds reflow the crawl into more pages, with blank
/// `" "` pages between paragraphs. A crawl page is therefore keyed `(record,
/// block, page)` apart from the record's other lines, so a block that grew on
/// the source disc neither shifts the lines after it nor pairs its own pages
/// across a paragraph gap ([`crawl_page_map`]).
pub(super) struct WalkTexts {
    /// The MAN's record shape - a lift only trusts the coordinates when both
    /// discs' MANs have the same one.
    shape: (usize, [i16; 3]),
    /// Text offset (first glyph byte) -> coordinate.
    by_off: BTreeMap<usize, Coord>,
    /// Coordinate -> text offset (line coordinates only).
    by_ord: BTreeMap<(usize, usize), usize>,
    /// `(record, block)` -> the block's page offsets, in order.
    crawls: BTreeMap<(usize, usize), Vec<usize>>,
}

/// Where a text lead sits in its record's walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coord {
    /// An ordinary line: `(record, ordinal among its non-crawl leads)`.
    Line(usize, usize),
    /// A crawl page: `(record, block ordinal, page ordinal)`.
    Page(usize, usize, usize),
}

/// Crawl-block opener `CC F8 80 N`: page count, or `None` for any other op.
pub(super) fn crawl_pages(man: &[u8], pc: usize) -> Option<usize> {
    match man.get(pc..pc + 4)? {
        [0xCC, 0xF8, 0x80, n] => Some(*n as usize),
        _ => None,
    }
}

/// Walk every record of `man` and collect its text leads. `None` when the
/// buffer is not a MAN.
pub(super) fn walk_texts(man: &[u8]) -> Option<WalkTexts> {
    let mf = man_section::parse(man).ok()?;
    let spans = field_disasm::man_script_spans(&mf, man);
    let mut w = WalkTexts {
        shape: (spans.len(), mf.header.partition_counts),
        by_off: BTreeMap::new(),
        by_ord: BTreeMap::new(),
        crawls: BTreeMap::new(),
    };
    for (rec, (_partition, _index, start, pc0, len)) in spans.into_iter().enumerate() {
        let end = start + len;
        let mut pc = start + pc0;
        let mut k = 0usize;
        let mut block = 0usize;
        // Pages still owed to the open crawl block.
        let mut owed = 0usize;
        while pc < end {
            let Ok(insn) = field_disasm::decode(man, pc) else {
                break;
            };
            if insn.size == 0 {
                break;
            }
            if let Some(n) = crawl_pages(man, pc) {
                owed = n;
                block += 1;
            } else if let Some(lead) = field_disasm::text_lead(man, &insn) {
                let off = lead + 1;
                if owed > 0 {
                    let pages = w.crawls.entry((rec, block - 1)).or_default();
                    w.by_off
                        .insert(off, Coord::Page(rec, block - 1, pages.len()));
                    pages.push(off);
                    owed -= 1;
                } else {
                    w.by_off.insert(off, Coord::Line(rec, k));
                    w.by_ord.insert((rec, k), off);
                    k += 1;
                }
            } else {
                // Anything else between the opener and its pages ends the block.
                owed = 0;
            }
            pc += insn.size;
        }
    }
    Some(w)
}

/// Pair the pages of one crawl block across two builds: for each target page,
/// the source page it takes, if any. Equal page counts pair one for one. A
/// reflowed block pairs its **text** pages one for one when the two builds
/// carry the same number of them - the extra pages a localization adds are
/// the blank paragraph gaps - and a blank target page stays as it is. Any
/// other disagreement pairs nothing: a crawl read from the wrong page is worse
/// than an English one.
pub(super) fn crawl_page_map(target: &[&[u8]], source: &[&[u8]]) -> Vec<Option<usize>> {
    if target.len() == source.len() {
        return (0..target.len()).map(Some).collect();
    }
    let text = |p: &[u8]| p.iter().any(|b| b.is_ascii_alphabetic() || *b >= 0x80);
    let src_text: Vec<usize> = (0..source.len()).filter(|&j| text(source[j])).collect();
    let tgt_text = target.iter().filter(|p| text(p)).count();
    if tgt_text != src_text.len() {
        return vec![None; target.len()];
    }
    let mut next = src_text.into_iter();
    target
        .iter()
        .map(|p| if text(p) { next.next() } else { None })
        .collect()
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

pub(super) fn man_of_scene(entry: &[u8]) -> Option<(usize, Vec<u8>)> {
    SceneManText::locate(entry).map(|m| (0, m.decoded))
}

pub(super) fn man_of_stream(entry: &[u8]) -> Option<(usize, Vec<u8>)> {
    StreamManText::locate(entry).map(|m| (m.man_range().start, m.man))
}

/// One dialog domain: how to list its scan-ordered segments (the positional
/// fallback) and how to reach its MAN (the structural pairing).
pub(super) struct DialogDomain {
    pub(super) seg_texts: SegTexts,
    pub(super) man_of: ManOf,
    /// The keyed offsets index the PROT entry itself (the `raw:` space), so
    /// the entry's own framing list can pair what the scan and the walk miss.
    pub(super) keys_are_entry_offsets: bool,
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
///
/// A line with **no prose of its own** - nothing but control tokens and
/// punctuation, e.g. a chest line reduced to its item-name token - is kept
/// even when the baseline carries it: the retail disc has the same bytes
/// because the construction is the same in both languages, not because the
/// translator skipped the line, and blanking it leaves the English line
/// (`the <item>!`) in the middle of a translated sentence. It carries no
/// text of the underlying build either.
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
            if e.translation.is_empty() || !has_prose(&e.translation) {
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

/// `true` when a markup line carries at least one letter outside its
/// `{..}` control tokens - words of its own, as opposed to a line built only
/// from tokens and punctuation.
pub(super) fn has_prose(markup: &str) -> bool {
    let mut depth = 0usize;
    for c in markup.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            c if depth == 0 && c.is_alphabetic() => return true,
            _ => {}
        }
    }
    false
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
pub(super) fn fill_dialog(
    target: &DiscPatcher,
    source: &DiscPatcher,
    entries: &mut [super::super::pack::Entry],
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
                (Some(uw), Some(pw)) if uw.shape == pw.shape => Some((ub, uw, pb, pm, pw, um)),
                _ => None,
            },
            _ => None,
        };
        let mut unpaired: Vec<(usize, usize)> = Vec::new();
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
            let structural = walks.as_ref().and_then(|(ub, uw, pb, pm, pw, um)| {
                let man_off = off.checked_sub(*ub)?;
                let pal_off = match *uw.by_off.get(&man_off)? {
                    Coord::Line(rec, k) => *pw.by_ord.get(&(rec, k))?,
                    Coord::Page(rec, b, i) => {
                        let (up, sp) = (uw.crawls.get(&(rec, b))?, pw.crawls.get(&(rec, b))?);
                        let texts = |man: &[u8], offs: &[usize]| -> Option<Vec<Vec<u8>>> {
                            offs.iter()
                                .map(|&o| text_at(man, o).map(<[u8]>::to_vec))
                                .collect()
                        };
                        let (ut, st) = (texts(um, up)?, texts(pm, sp)?);
                        let ur: Vec<&[u8]> = ut.iter().map(Vec::as_slice).collect();
                        let sr: Vec<&[u8]> = st.iter().map(Vec::as_slice).collect();
                        sp[crawl_page_map(&ur, &sr).get(i).copied().flatten()?]
                    }
                };
                text_at(pm, pal_off).map(|t| (pal_off + pb, t))
            });
            // A crawl page the walk placed pairs structurally or not at all:
            // the scan ordinal of a reflowed block names another page.
            let is_page = walks.as_ref().is_some_and(|(ub, uw, ..)| {
                off.checked_sub(*ub)
                    .and_then(|o| uw.by_off.get(&o))
                    .is_some_and(|c| matches!(c, Coord::Page(..)))
            });
            let scan = positional;
            let positional = if is_page { None } else { positional };
            let chosen = match (structural, positional) {
                (Some((pal_off, text)), _) => {
                    stats.structural += 1;
                    if scan.is_none_or(|(o, _)| o != pal_off) {
                        stats.shifted += 1;
                    }
                    Some(text)
                }
                (None, Some((_, text))) => {
                    stats.positional += 1;
                    Some(text)
                }
                (None, None) => {
                    unpaired.push((m, off));
                    None
                }
            };
            if let Some(text) = chosen {
                entries[m].translation = markup::decode(text);
                *paired += 1;
            }
        }
        // Last resort for a raw carrier the walk does not cover: the entry's
        // every `0x1F .. 0x00` framing, quality gate off. A translated line
        // too short for the gate (a chest line reduced to its item token)
        // shifts the gated scan by one on that side only; the ungated lists
        // stay the same length when only the text changed. Paired only when
        // they do, and only where the script byte ahead of both leads agrees.
        if domain.keys_are_entry_offsets && !unpaired.is_empty() {
            let (uf, pf) = (framings(&usa_entry), framings(&pal_entry));
            if uf.len() == pf.len() {
                let ord: BTreeMap<usize, usize> =
                    uf.iter().enumerate().map(|(k, (o, _))| (*o, k)).collect();
                for (m, off) in unpaired.drain(..) {
                    let Some((pal_off, text)) = ord.get(&off).map(|&k| &pf[k]) else {
                        continue;
                    };
                    let lead_agrees =
                        off >= 2 && *pal_off >= 2 && usa_entry[off - 2] == pal_entry[pal_off - 2];
                    if lead_agrees && !text.is_empty() {
                        entries[m].translation = markup::decode(text);
                        stats.positional += 1;
                        *paired += 1;
                    }
                }
            }
        }
    }
    Ok(stats)
}
