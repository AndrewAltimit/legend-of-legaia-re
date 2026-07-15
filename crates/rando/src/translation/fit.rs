//! Fit-rate measurement for an official localization against the USA target,
//! under two budget models (text-free - counts only):
//!
//! - **per-string** (the old same-size constraint): a line fits iff its encoded
//!   bytes are `<=` its own USA segment span.
//! - **per-MAN** (the generalized rewriter): a whole scene MAN fits iff *all*
//!   its official lines, grown to full length, relocate + validate + recompress
//!   within the MAN's on-disc compressed footprint (same LBA, no disc relayout).
//!   A MAN that overflows is a **residual sector-crosser** - its deficit is the
//!   compressed bytes over budget (the extra it would need, which the PAL disc
//!   supplied by growing the entry one sector).
//!
//! Raw event-script carriers are uncompressed in place, so they have no growth
//! path and stay per-string bounded; they are reported separately.
//!
//! This is the honest measurement behind `docs/tooling/pal-localizations.md`'s
//! fit-rate section: it runs the real rewriter, so a "fits" verdict means the
//! importer would actually write that MAN losslessly.

use std::collections::BTreeMap;

use anyhow::Result;

use legaia_asset::man_edit::{self, TextEdit};

use crate::disc::DiscPatcher;

use super::export::SceneManText;
use super::lift;
use super::markup::{self, Target};
use super::pack::LanguagePack;
use super::segments;

/// Fit tallies for one language (counts only - no text).
#[derive(Debug, Clone, Default)]
pub struct FitReport {
    pub language: String,
    /// Pooled SCUS name strings (per-string CString budget only).
    pub name_lines: usize,
    pub name_perstring_fit: usize,
    /// MAN dialog lines.
    pub man_lines: usize,
    /// Lines whose encoded text fits its own USA span (per-string budget).
    pub man_perstring_fit: usize,
    /// Scene MAN PROT entries carrying filled dialog.
    pub man_entries: usize,
    /// MANs that grow fully in place (all lines lossless, same LBA).
    pub man_entries_fit: usize,
    /// MANs whose grown dialog overflows the compressed footprint by a known
    /// deficit (would need +1 sector).
    pub man_entries_residual_overflow: usize,
    /// MANs the rewriter can't grow structurally (an abs-ref op / section-region
    /// segment / validation divergence) - also residual.
    pub man_entries_residual_structural: usize,
    /// Lines living in a fully-fitting MAN (the per-MAN-budget lossless set).
    pub man_lines_perman_fit: usize,
    /// Lines living in a residual MAN (would need abbreviation or +1 sector).
    pub man_lines_residual: usize,
    /// Per-overflow-residual-MAN compressed deficit in bytes.
    pub residual_deficits: Vec<usize>,
    /// Raw event-script carrier lines (per-string budget only).
    pub raw_lines: usize,
    pub raw_perstring_fit: usize,
}

impl FitReport {
    /// `true` when every overflow-residual MAN's deficit is within one 2048-byte
    /// sector (the "+1 sector each" class the PAL disc grew).
    pub fn all_residuals_within_one_sector(&self) -> bool {
        self.residual_deficits.iter().all(|&d| d <= 2048)
    }

    pub fn residual_deficit_max(&self) -> usize {
        self.residual_deficits.iter().copied().max().unwrap_or(0)
    }
}

/// Parse the PROT entry index of a `man:<idx>:0x..` / `raw:<idx>:0x..` key.
fn key_entry_index(key: &str) -> Option<usize> {
    key.split(':').nth(1)?.parse().ok()
}

/// Parse the decompressed-domain offset of a `man:/raw:` key.
fn key_off(key: &str) -> Option<usize> {
    usize::from_str_radix(key.rsplit(":0x").next()?, 16).ok()
}

/// Encode a filled entry's translation for `target`, or `None` if it can't
/// encode (a lifted line with a byte the codec rejects - never expected).
fn enc(pack_text: &str, target: Target) -> Option<Vec<u8>> {
    markup::encode(pack_text, target).ok()
}

/// Measure the fit of `pack` (a lifted working pack keyed to `target`) under
/// both budget models.
pub fn measure(target: &DiscPatcher, pack: &LanguagePack) -> Result<FitReport> {
    let mut rep = FitReport {
        language: pack.language.clone(),
        ..Default::default()
    };

    // ---- Pooled name strings: per-string CString budget ----
    for (name, entries) in pack.sections.iter() {
        let is_name = matches!(
            name,
            "items" | "item_types" | "spells" | "arts" | "accessory_passives" | "party_names"
        );
        if !is_name {
            continue;
        }
        for e in entries.iter().filter(|e| e.is_filled()) {
            rep.name_lines += 1;
            if enc(&e.translation, Target::CString).is_some_and(|b| b.len() <= e.budget) {
                rep.name_perstring_fit += 1;
            }
        }
    }

    // ---- Raw carriers: per-string Segment budget (no growth path) ----
    for e in pack.sections.inline_text.iter().filter(|e| e.is_filled()) {
        rep.raw_lines += 1;
        if enc(&e.translation, Target::Segment).is_some_and(|b| b.len() <= e.budget) {
            rep.raw_perstring_fit += 1;
        }
    }

    // ---- MAN dialog: per-string + per-MAN growth ----
    let mut by_entry: BTreeMap<usize, Vec<&super::pack::Entry>> = BTreeMap::new();
    for e in pack.sections.scene_dialog.iter().filter(|e| e.is_filled()) {
        if let Some(idx) = key_entry_index(&e.key) {
            by_entry.entry(idx).or_default().push(e);
        }
    }

    for (prot, lines) in by_entry {
        let Ok(entry_bytes) = target.read_entry(prot) else {
            continue;
        };
        let Some(man) = SceneManText::locate(&entry_bytes) else {
            continue;
        };
        rep.man_entries += 1;
        let n = lines.len();
        rep.man_lines += n;

        // Build the text edits + per-string tally.
        let mut edits: Vec<TextEdit> = Vec::new();
        for e in &lines {
            let (Some(off), Some(translated)) =
                (key_off(&e.key), enc(&e.translation, Target::Segment))
            else {
                continue;
            };
            let Some(term) = segments::walk_to_terminator(&man.decoded, off) else {
                continue;
            };
            let old_len = term - off;
            if translated.len() <= old_len {
                rep.man_perstring_fit += 1;
            }
            edits.push(TextEdit {
                offset: off,
                old_len,
                new_bytes: translated,
            });
        }

        // Per-MAN growth: rebuild with every line at full length, validate,
        // recompress within the MAN's compressed footprint.
        match grow_and_measure(&man, &edits) {
            GrowFit::Fit => {
                rep.man_entries_fit += 1;
                rep.man_lines_perman_fit += n;
            }
            GrowFit::Overflow(deficit) => {
                rep.man_entries_residual_overflow += 1;
                rep.man_lines_residual += n;
                rep.residual_deficits.push(deficit);
            }
            GrowFit::Structural => {
                rep.man_entries_residual_structural += 1;
                rep.man_lines_residual += n;
            }
        }
    }

    Ok(rep)
}

enum GrowFit {
    /// Grows losslessly within the compressed footprint.
    Fit,
    /// Recompresses `deficit` bytes over the footprint (would need +1 sector).
    Overflow(usize),
    /// Can't be grown structurally (abs-ref op / section-region / validation).
    Structural,
}

fn grow_and_measure(man: &SceneManText, edits: &[TextEdit]) -> GrowFit {
    let Ok(grown) = man_edit::apply_text_edits(&man.decoded, edits) else {
        return GrowFit::Structural;
    };
    if !man_edit::text_edits_preserve_scripts(&man.decoded, &grown) {
        return GrowFit::Structural;
    }
    let best = legaia_lzs::compress(&grown)
        .len()
        .min(legaia_lzs::compress_optimal(&grown).len());
    if best <= man.compressed_budget {
        GrowFit::Fit
    } else {
        GrowFit::Overflow(best - man.compressed_budget)
    }
}

/// Lift `source` onto `target` and measure the resulting pack's fit.
pub fn lift_and_measure(target: &DiscPatcher, source: &DiscPatcher) -> Result<FitReport> {
    let (pack, _) = lift::lift_official(target, source)?;
    measure(target, &pack)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_helpers() {
        assert_eq!(key_entry_index("man:874:0x1a"), Some(874));
        assert_eq!(key_off("man:874:0x1a"), Some(0x1a));
        assert_eq!(key_off("raw:12:0x0"), Some(0));
    }

    #[test]
    fn residual_sector_predicate() {
        let mut r = FitReport {
            residual_deficits: vec![10, 900, 2048],
            ..Default::default()
        };
        assert!(r.all_residuals_within_one_sector());
        r.residual_deficits.push(2049);
        assert!(!r.all_residuals_within_one_sector());
    }
}
