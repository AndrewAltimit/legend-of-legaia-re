//! Dialog coverage: does the export carry every line the game can draw?
//!
//! The denominator is the **script walk**, not the exporter's own scanner. For
//! every scene MAN (LZS scene bundle) and every streaming scene's leading MAN
//! chunk, each partition record's script is decoded from its first opcode
//! (`man_edit::record_script_windows`), and every instruction that carries a
//! line - a bare line, an op-`0x49` sub-0 inline MES, an op-`0x4C` `E1`
//! balloon - contributes that line's lead. A lead the walk reaches is dialog
//! whatever it reads like; the report then says which of those leads the
//! exported pack carries, and why each one it lacks is missing.
//!
//! Three more counts ride along, none of which the walk can decide:
//!
//! - **unreached candidates** - text-shaped lines in a MAN that no walk
//!   reaches (a record the decoder stops early in). Upper bound on what a
//!   walk defect could be hiding;
//! - **foreign lines** - count-led Shift-JIS lines inside a Latin build's
//!   MAN (untranslated leftovers the Latin text engine cannot draw);
//! - **shop names** - the vendor name an op-`0x49` shop record carries,
//!   which is not a `0x1F` line and is not in the pack.
//!
//! Counts and offsets only - no text - so the report is safe to log.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::Serialize;

use legaia_asset::{field_disasm, man_edit, shop_stock};

use crate::disc::DiscPatcher;

use super::build::{self, TextCodec};
use super::export::{self, SceneManText};
use super::segments;
use super::sjis;
use super::stream_man::StreamManText;

/// One MAN-bearing carrier's coverage.
#[derive(Debug, Clone, Serialize)]
pub struct CarrierCoverage {
    /// PROT entry (extraction index).
    pub entry: usize,
    /// CDNAME block name.
    pub scene: String,
    /// `scene_man` (LZS scene bundle, `man:` keys) or `stream_man` (a
    /// streaming scene's uncompressed MAN chunk, `raw:` keys).
    pub kind: &'static str,
    /// Line leads the script walk reaches.
    pub walked: usize,
    /// Walked leads the pack carries.
    pub exported: usize,
    /// Walked leads the pack lacks, with the reason (key offset, reason).
    pub missing: Vec<(usize, &'static str)>,
    /// Lines the pack carries that the walk does not reach (the scanner's
    /// quality gate admitted them).
    pub exported_unwalked: usize,
    /// Text-shaped lines no walk reaches and the pack lacks.
    pub unreached_candidates: usize,
    /// Count-led Shift-JIS lines in a Latin build's MAN.
    pub foreign_lines: usize,
    /// Op-`0x49` shop records (vendor names; not in the pack).
    pub shop_names: usize,
}

/// The whole report.
#[derive(Debug, Clone, Serialize)]
pub struct CoverageReport {
    /// Boot executable.
    pub exe: String,
    /// `latin` or `shift_jis`.
    pub codec: &'static str,
    /// Entries per pack section, as the export emits them.
    pub sections: BTreeMap<String, usize>,
    /// Raw-carrier lines outside any MAN chunk (Latin prose-gated scan; no
    /// walk decides these).
    pub raw_unwalked_lines: usize,
    /// Per carrier, in entry order.
    pub carriers: Vec<CarrierCoverage>,
}

impl CoverageReport {
    /// `(walked, exported, missing)` summed over every carrier.
    pub fn totals(&self) -> (usize, usize, usize) {
        self.carriers.iter().fold((0, 0, 0), |(w, e, m), c| {
            (w + c.walked, e + c.exported, m + c.missing.len())
        })
    }
}

/// Lead offsets the Latin walk reaches: the `0x1F` byte of every line an
/// instruction on a record's clean walk carries.
pub fn latin_walk_leads(man: &[u8]) -> Vec<usize> {
    let mut leads = BTreeSet::new();
    for (pc0, end) in man_edit::record_script_windows(man) {
        let mut pc = pc0;
        while pc < end {
            let Ok(insn) = field_disasm::decode(man, pc) else {
                break;
            };
            if insn.size == 0 {
                break;
            }
            if let Some(lead) = field_disasm::text_lead(man, &insn) {
                leads.insert(lead);
            }
            pc += insn.size;
        }
    }
    leads.into_iter().collect()
}

/// Why a walked Latin lead has no pack entry.
fn latin_reason(man: &[u8], lead: usize) -> &'static str {
    match segments::walk_to_terminator(man, lead + 1) {
        None => "unterminated",
        Some(t) if man[t] != 0 => "non-NUL terminator",
        Some(t) if man[lead + 1..t].iter().all(|&b| b == b' ') => "blank line",
        Some(_) => "rejected",
    }
}

/// Where a record's clean walk stops: the window end, or the first byte the
/// build's decoder cannot step over.
fn walk_stop(man: &[u8], codec: TextCodec, pc0: usize, end: usize) -> usize {
    let mut pc = pc0;
    while pc < end {
        let size = match codec {
            TextCodec::Latin { .. } => match field_disasm::decode(man, pc) {
                Ok(insn) if insn.size > 0 => insn.size,
                _ => break,
            },
            TextCodec::ShiftJis => match sjis::step(man, pc) {
                Some((size, _)) => size,
                None => break,
            },
        };
        pc += size;
    }
    pc.min(end)
}

/// Text-shaped lines in the part of each record's script window its clean
/// walk never reaches (past the byte the decoder stops at) that the pack
/// lacks. What a walk defect could be hiding; zero when every walk runs to
/// its record's end.
fn unreached(man: &[u8], codec: TextCodec, covered: &BTreeSet<usize>) -> usize {
    let tails: Vec<(usize, usize)> = man_edit::record_script_windows(man)
        .into_iter()
        .map(|(pc0, end)| (walk_stop(man, codec, pc0, end), end))
        .filter(|(stop, end)| stop < end)
        .collect();
    let mut n = 0;
    for (stop, end) in tails {
        let tail = &man[..end];
        match codec {
            TextCodec::Latin { allow_high } => {
                let part = &tail[stop..];
                n += segments::scan_ext(part, allow_high)
                    .into_iter()
                    .filter(|s| {
                        !covered.contains(&(stop + s.text_off))
                            && segments::is_prose(&part[s.text_off..s.text_off + s.len])
                    })
                    .count();
            }
            TextCodec::ShiftJis => {
                // Count-led runs of four or more plain characters, skipping
                // over each run found.
                let mut i = stop;
                while i < end {
                    match sjis::line_tokens(tail, i) {
                        Some(t)
                            if t.len() >= 8
                                && t.as_chunks::<2>()
                                    .0
                                    .iter()
                                    .all(|p| sjis::is_char_lead(p[0]))
                                && sjis::qualifies(t) =>
                        {
                            if !covered.contains(&(i + 1)) {
                                n += 1;
                            }
                            i += 1 + t.len();
                        }
                        _ => i += 1,
                    }
                }
            }
        }
    }
    n
}

/// Coverage of one decompressed MAN whose keys sit at `base + offset`.
fn man_coverage(
    man: &[u8],
    codec: TextCodec,
    base: usize,
    exported: &BTreeSet<usize>,
) -> (
    usize,
    usize,
    Vec<(usize, &'static str)>,
    usize,
    usize,
    usize,
) {
    let (leads, reason): (Vec<usize>, Box<dyn Fn(usize) -> &'static str>) = match codec {
        TextCodec::Latin { .. } => (latin_walk_leads(man), Box::new(|l| latin_reason(man, l))),
        TextCodec::ShiftJis => (sjis::walk_leads(man), Box::new(|_| "tokens not text")),
    };
    let walked: BTreeSet<usize> = leads.iter().map(|&l| l + 1).collect();
    let mut missing = Vec::new();
    let mut hit = 0;
    for &lead in &leads {
        if exported.contains(&(base + lead + 1)) {
            hit += 1;
        } else {
            missing.push((base + lead + 1, reason(lead)));
        }
    }
    let exported_unwalked = exported
        .iter()
        .filter(|&&k| k >= base && k < base + man.len() && !walked.contains(&(k - base)))
        .count();
    let covered: BTreeSet<usize> = exported
        .iter()
        .filter_map(|&k| k.checked_sub(base))
        .chain(walked.iter().copied())
        .collect();
    let unreached = unreached(man, codec, &covered);
    let foreign = match codec {
        TextCodec::Latin { .. } => sjis::walk_leads(man)
            .into_iter()
            .filter(|&l| {
                sjis::line_tokens(man, l).is_some_and(|t| t.len() >= 4 && sjis::qualifies(t))
            })
            .count(),
        TextCodec::ShiftJis => 0,
    };
    (
        leads.len(),
        hit,
        missing,
        exported_unwalked,
        unreached,
        foreign,
    )
}

/// Parse a `man:` / `raw:` key into `(prefix, entry, offset)`.
fn parse_dialog_key(key: &str) -> Option<(&str, usize, usize)> {
    let mut it = key.split(':');
    let prefix = it.next()?;
    let entry = it.next()?.parse().ok()?;
    let off = usize::from_str_radix(it.next()?.strip_prefix("0x")?, 16).ok()?;
    Some((prefix, entry, off))
}

/// Measure the export of `patcher`'s disc against its script walk.
pub fn measure(patcher: &DiscPatcher) -> Result<CoverageReport> {
    let build = build::detect(patcher);
    let pack = export::export_pack(patcher)?;
    let mut man_keys: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    let mut raw_keys: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    let mut sections = BTreeMap::new();
    for (name, entries) in pack.sections.iter() {
        sections.insert(name.to_string(), entries.len());
        for e in entries {
            match parse_dialog_key(&e.key) {
                Some(("man", entry, off)) => {
                    man_keys.entry(entry).or_default().insert(off);
                }
                Some(("raw", entry, off)) => {
                    raw_keys.entry(entry).or_default().insert(off);
                }
                _ => {}
            }
        }
    }

    let cdname = patcher.cdname();
    let scene_of = |idx: usize| -> String {
        cdname
            .as_ref()
            .and_then(|m| legaia_prot::cdname::block_for_extraction_index(m, idx as u32))
            .unwrap_or("?")
            .to_string()
    };
    let empty = BTreeSet::new();
    let mut carriers = Vec::new();
    let mut raw_unwalked_lines = 0;
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        let man_found = SceneManText::locate(&entry);
        let stream = if man_found.is_none() {
            match build.codec {
                TextCodec::Latin { .. } => StreamManText::locate(&entry),
                TextCodec::ShiftJis => StreamManText::locate_structural(&entry),
            }
        } else {
            None
        };
        let (kind, man, base, keys) = match (&man_found, &stream) {
            (Some(m), _) => (
                "scene_man",
                m.decoded.as_slice(),
                0,
                man_keys.get(&idx).unwrap_or(&empty),
            ),
            (None, Some(s)) => (
                "stream_man",
                s.man.as_slice(),
                s.man_range().start,
                raw_keys.get(&idx).unwrap_or(&empty),
            ),
            (None, None) => {
                raw_unwalked_lines += raw_keys.get(&idx).map_or(0, BTreeSet::len);
                continue;
            }
        };
        if kind == "stream_man" {
            let r = s_range(&stream);
            raw_unwalked_lines += keys.iter().filter(|k| !r.contains(k)).count();
        }
        let (walked, exported, missing, exported_unwalked, unreached_candidates, foreign_lines) =
            man_coverage(man, build.codec, base, keys);
        let shop_names = match (build.codec, &man_found) {
            (TextCodec::Latin { .. }, Some(_)) => {
                shop_stock::locate(&entry, None).map_or(0, |s| s.records.len())
            }
            _ => 0,
        };
        carriers.push(CarrierCoverage {
            entry: idx,
            scene: scene_of(idx),
            kind,
            walked,
            exported,
            missing,
            exported_unwalked,
            unreached_candidates,
            foreign_lines,
            shop_names,
        });
    }
    Ok(CoverageReport {
        exe: build.exe.clone(),
        codec: match build.codec {
            TextCodec::Latin { .. } => "latin",
            TextCodec::ShiftJis => "shift_jis",
        },
        sections,
        raw_unwalked_lines,
        carriers,
    })
}

fn s_range(stream: &Option<StreamManText>) -> std::ops::Range<usize> {
    stream.as_ref().map_or(0..0, StreamManText::man_range)
}
