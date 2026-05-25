//! Semantic labels for cataloged TIMs.
//!
//! The TIM catalog ([`crate::tim_catalog`] for raw entries,
//! [`crate::tim_deep_catalog`] for TIMs inside LZS-compressed sections) records
//! *where* every texture lives and its dimensions, but not *what* it is. This
//! module is a curated label table that answers the "what" for the textures we
//! have identified.
//!
//! ## A fingerprint-keyed table
//!
//! Each row of the committed table ([`data/tim_categories.tsv`]) maps a TIM
//! content fingerprint - the FNV-1a-64 the catalogs already record - to a
//! controlled-vocabulary label (and an optional human note). Keying by content
//! rather than by catalog id means a single label propagates to every id that
//! shares those bytes: duplicate textures, and textures aliased across several
//! overlapping PROT entries, all resolve to one row, and the same row serves
//! both the raw and the deep tier.
//!
//! ## The labels are our own annotations
//!
//! A label is either a **coarse visual category** assigned by inspecting the
//! decoded thumbnail (`environment`, `terrain`, `foliage`, `character`,
//! `ui-text`, `effect`, `other`) or a **precise role** for a texture pinned by
//! reverse engineering (the boot/title/menu textures, whose loader sites and
//! byte offsets are known). Both are our own observations - never an asset
//! string and never pixel data - so the table is safe to commit, like the
//! ground-truth gamedata tables.
//!
//! Earlier revisions tried to derive an "NPC palette" label structurally from
//! the CLUT load position (`fb=(0, 479)`). That is **unsound**: nearly every
//! 256x256 4bpp scene/field texture page parks its CLUT in that same bottom
//! VRAM band, so the rule conflated floors / walls / terrain with NPC colour
//! tables. Labels are now content-keyed observations, not a CLUT heuristic.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The committed curated label table: tab-separated `fnv1a<TAB>label[<TAB>note]`
/// with a header line and optional `#` comments. Embedded so the lookup works
/// everywhere the catalog does (CLI, tests, the in-browser WASM viewer) without
/// a side file.
const TABLE_TSV: &str = include_str!("data/tim_categories.tsv");

/// The controlled label vocabulary. Coarse visual categories first, then the
/// precise roles of the byte-exact RE pins (more specific than a coarse
/// bucket). Any label in the table outside this set fails [`table_is_valid`].
pub const VALID_LABELS: &[&str] = &[
    // Coarse visual categories.
    "environment", // floor / wall / structure / interior
    "terrain",     // world-map / overworld ground
    "foliage",     // trees / plants / nature
    "character",   // NPC / party / creature sprite sheets
    "ui-text",     // menu chrome / fonts / icons / HUD
    "effect",      // particles / spell / battle FX
    "other",       // unclassified or ambiguous
    // Precise byte-exact pins (reverse-engineered loader sites).
    "menu glyph atlas",
    "main-title sprite sheet",
    "publisher logo",
    "load-screen UI sheet",
    "load-screen portrait",
    "load-screen empty-slot frame",
];

/// Parse one table line into `(fnv1a, label)`, skipping blanks / comments / the
/// header. Returns `None` for lines that don't carry a row.
fn parse_row(line: &str) -> Option<(u64, &str)> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line.is_empty() || line.starts_with('#') || line.starts_with("fnv1a") {
        return None;
    }
    let mut cols = line.split('\t');
    let fnv_hex = cols.next()?.trim();
    let label = cols.next()?.trim();
    if label.is_empty() {
        return None;
    }
    let fnv = u64::from_str_radix(fnv_hex, 16).ok()?;
    Some((fnv, label))
}

/// Lazily-built `fingerprint -> label` map over the embedded table. Labels are
/// `&'static str` slices of the embedded text.
fn table() -> &'static HashMap<u64, &'static str> {
    static MAP: OnceLock<HashMap<u64, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for line in TABLE_TSV.lines() {
            if let Some((fnv, label)) = parse_row(line) {
                m.insert(fnv, label);
            }
        }
        m
    })
}

/// The curated semantic label for a TIM with this content fingerprint, or
/// `None` if it isn't in the table yet.
pub fn label_for(fnv1a: u64) -> Option<&'static str> {
    table().get(&fnv1a).copied()
}

/// Number of curated rows (distinct fingerprints) in the table.
pub fn table_len() -> usize {
    table().len()
}

/// Validate the embedded table: every row parses, every fingerprint is unique,
/// and every label is in [`VALID_LABELS`]. Returns the offending line on
/// failure so the test message points at it.
pub fn table_is_valid() -> Result<(), String> {
    let mut seen: HashMap<u64, &str> = HashMap::new();
    for (n, line) in TABLE_TSV.lines().enumerate() {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("fnv1a") {
            continue;
        }
        let Some((fnv, label)) = parse_row(line) else {
            return Err(format!("line {}: unparseable row: {trimmed:?}", n + 1));
        };
        if !VALID_LABELS.contains(&label) {
            return Err(format!("line {}: unknown label {label:?}", n + 1));
        }
        if let Some(prev) = seen.insert(fnv, label)
            && prev != label
        {
            return Err(format!(
                "line {}: fingerprint {fnv:016x} relabeled {prev:?} -> {label:?}",
                n + 1
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_table_is_valid() {
        table_is_valid().expect("curated label table");
        assert!(table_len() > 0, "table should have rows");
    }

    #[test]
    fn byte_exact_pins_resolve() {
        // The reverse-engineered boot/title/menu pins (by content fingerprint).
        assert_eq!(label_for(0x4bf1_abb7_2854_3d02), Some("menu glyph atlas"));
        assert_eq!(
            label_for(0xa285_61c0_84c1_a351),
            Some("main-title sprite sheet")
        );
        assert_eq!(
            label_for(0x78f5_61f4_b6af_5a88),
            Some("load-screen UI sheet")
        );
        assert_eq!(
            label_for(0x7498_8d0f_fc9c_c738),
            Some("load-screen portrait")
        );
        assert_eq!(
            label_for(0x5c73_3293_87c9_1106),
            Some("load-screen empty-slot frame")
        );
        assert_eq!(label_for(0x4819_b00d_5495_ff72), Some("publisher logo"));
    }

    #[test]
    fn unknown_fingerprint_is_none() {
        assert_eq!(label_for(0xdead_beef_dead_beef), None);
    }

    #[test]
    fn parse_row_skips_header_and_comments() {
        assert_eq!(parse_row("fnv1a\tlabel\tnote"), None);
        assert_eq!(parse_row("# a comment"), None);
        assert_eq!(parse_row(""), None);
        assert_eq!(
            parse_row("4bf1abb728543d02\tmenu glyph atlas\t"),
            Some((0x4bf1_abb7_2854_3d02, "menu glyph atlas"))
        );
    }
}
