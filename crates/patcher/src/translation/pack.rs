//! Language-pack YAML schema (serde types) + coverage stats.
//!
//! A pack is one YAML document: a small header (format tag, language code,
//! contributors, notes) plus fixed, ordered sections of [`Entry`] lists. Every
//! entry carries a stable provenance `key` (where on the disc the text lives),
//! an initially empty `translation`, and the byte `budget` an encoded
//! translation must fit (same-size in-place patching - see
//! `docs/tooling/translation.md`).
//!
//! Only entries whose `translation` is non-empty are ever written to a disc;
//! everything else is left byte-identical.
//!
//! # Two pack shapes
//!
//! | Shape | `source:` | Contains | Where it lives |
//! |---|---|---|---|
//! | **working** | yes | the disc's own text, for the translator to read | the user's machine (never redistributed) |
//! | **distributable** | no | only *new* text keyed by disc coordinates | shareable / shippable |
//!
//! [`LanguagePack::strip_sources`] converts working -> distributable: it drops
//! every `source:` field and every unfilled entry, leaving a pure
//! `key -> translation` lookup table. A distributable pack therefore carries
//! none of the original script - the `key` is a disc coordinate (a VA or a
//! PROT-entry byte offset) and the `budget` is a byte count.
//!
//! Because a distributable pack has no `source` to check itself against, its
//! `budget` is only a **hint**: [`super::import`] re-derives the real byte
//! budget from the disc it is patching and rejects any entry that doesn't fit
//! (and any entry whose on-disc length disagrees with the hint - the wrong-disc
//! guard that `source` provides for a working pack).

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Format tag in the pack header; bump on breaking schema changes.
pub const PACK_FORMAT: &str = "legaia-text-pack-v1";

/// One translatable string.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    /// Stable provenance key. Shapes:
    /// - `scus:str:0x<va>` - NUL-terminated string in `SCUS_942.54` at that
    ///   virtual address (item / spell / art / passive name pools);
    /// - `scus:party:<n>` - fixed 10-byte name field of new-game roster
    ///   record `n`;
    /// - `man:<entry>:0x<off>` - `0x1F`-lead dialog segment at byte `off`
    ///   inside PROT entry `entry`'s **decompressed** scene MAN;
    /// - `raw:<entry>:0x<off>` - `0x1F`-lead text segment at byte `off`
    ///   inside PROT entry `entry`'s raw bytes (event-script prescripts,
    ///   streaming MAN carriers);
    /// - `ui:<entry>:0x<va>` - NUL-terminated UI string at virtual address
    ///   `va` inside PROT overlay entry `entry` (menu / battle labels; the
    ///   file offset is `va - base_va`, see [`super::ui`]).
    pub key: String,
    /// Human context (scene name, table ids, neighbours). Not machine-read.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub context: String,
    /// Source (US English) text in markup form. **Absent in a distributable
    /// pack** - that shape ships only new text, so there is nothing to compare
    /// against and [`super::import`] derives the budget from the disc instead.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// Target-language text in markup form. Empty = leave the disc untouched.
    #[serde(default)]
    pub translation: String,
    /// Maximum **encoded byte** length a translation may occupy. The encoder
    /// output must satisfy `len <= budget` (shorter is fine - dialog segments
    /// are space-padded, strings are re-terminated).
    ///
    /// Authoritative in a working pack (it was measured off the disc during
    /// export); a *hint* in a distributable pack, where import re-measures the
    /// target on the disc being patched and uses the hint only to detect a
    /// disc/pack mismatch.
    pub budget: usize,
}

impl Entry {
    /// `true` when a translator has filled this entry in.
    pub fn is_filled(&self) -> bool {
        !self.translation.trim().is_empty()
    }
}

/// Fixed section set, serialized in this order. Each is one text population
/// with a uniform patch mechanism.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sections {
    /// Item names (`SCUS_942.54` item-name table, MES `{c2:xx}` target).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Entry>,
    /// Shared item "type" strings (second pointer of each item record).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub item_types: Vec<Entry>,
    /// Spell / magic names (MES `{c3:xx}` target).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spells: Vec<Entry>,
    /// Tactical Arts names (MES `{c5:xx}` target).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arts: Vec<Entry>,
    /// Accessory passive-effect names + descriptions (Goods menu).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accessory_passives: Vec<Entry>,
    /// New-game party names (fixed 10-byte fields; `{c1:xx}` renders these).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub party_names: Vec<Entry>,
    /// NPC / event dialog inside the LZS-compressed scene-bundle MANs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scene_dialog: Vec<Entry>,
    /// Dialog + narration in raw (uncompressed) PROT carriers: the v12
    /// event-script prescripts and the streaming-MAN dungeon scenes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline_text: Vec<Entry>,
    /// Overlay-resident UI menu strings (pause-menu / options / shop / equip /
    /// status command labels + in-battle system messages). See [`super::ui`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ui_menu: Vec<Entry>,
}

impl Sections {
    /// Iterate `(section_name, entries)` in serialization order.
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &[Entry])> {
        [
            ("items", self.items.as_slice()),
            ("item_types", self.item_types.as_slice()),
            ("spells", self.spells.as_slice()),
            ("arts", self.arts.as_slice()),
            ("accessory_passives", self.accessory_passives.as_slice()),
            ("party_names", self.party_names.as_slice()),
            ("scene_dialog", self.scene_dialog.as_slice()),
            ("inline_text", self.inline_text.as_slice()),
            ("ui_menu", self.ui_menu.as_slice()),
        ]
        .into_iter()
    }

    /// Mutable view of every section, in serialization order.
    pub fn each_mut(&mut self) -> [&mut Vec<Entry>; 9] {
        [
            &mut self.items,
            &mut self.item_types,
            &mut self.spells,
            &mut self.arts,
            &mut self.accessory_passives,
            &mut self.party_names,
            &mut self.scene_dialog,
            &mut self.inline_text,
            &mut self.ui_menu,
        ]
    }

    /// Total entry count across all sections.
    pub fn total(&self) -> usize {
        self.iter().map(|(_, e)| e.len()).sum()
    }

    /// Entries a translator has filled in.
    pub fn filled(&self) -> usize {
        self.iter()
            .map(|(_, e)| e.iter().filter(|e| e.is_filled()).count())
            .sum()
    }
}

/// A whole language pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguagePack {
    /// Must equal [`PACK_FORMAT`].
    pub format: String,
    /// BCP-47-ish language code of the `translation` fields
    /// (`en` for a fresh export; `fr de es it pt-BR ja ru zh ko ...`).
    pub language: String,
    /// The disc the pack was exported from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub game: String,
    /// Credited translators / editors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributors: Vec<String>,
    /// Free-form pack notes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub sections: Sections,
}

impl LanguagePack {
    /// New empty pack with the standard header.
    pub fn new(language: &str) -> Self {
        Self {
            format: PACK_FORMAT.to_string(),
            language: language.to_string(),
            game: "Legend of Legaia (USA) SCUS-94254".to_string(),
            contributors: Vec::new(),
            notes: String::new(),
            sections: Sections::default(),
        }
    }

    /// Parse + format-check a pack from YAML text.
    pub fn from_yaml(text: &str) -> Result<Self> {
        let pack: LanguagePack = serde_yaml::from_str(text).context("parse language-pack YAML")?;
        if pack.format != PACK_FORMAT {
            bail!(
                "unsupported pack format {:?} (this tool reads {PACK_FORMAT:?})",
                pack.format
            );
        }
        Ok(pack)
    }

    /// Serialize to YAML. Hand-rolled emitter rather than `serde_yaml`
    /// because every string scalar must be **quoted**: game text like `Yes`,
    /// `No`, `On` or `1000G` would otherwise be re-read as booleans/numbers
    /// by the YAML 1.1 parsers (PyYAML et al.) translators commonly script
    /// with, corrupting the pack on their side. `serde_yaml` offers no
    /// force-quote switch; single-quoted style round-trips through every
    /// YAML 1.1/1.2 implementation.
    pub fn to_yaml(&self) -> Result<String> {
        fn q(s: &str) -> String {
            // YAML single-quoted scalar: only `'` needs escaping (doubled).
            format!("'{}'", s.replace('\'', "''"))
        }
        let mut out = String::with_capacity(4096 + self.sections.total() * 160);
        out.push_str(&format!("format: {}\n", q(&self.format)));
        out.push_str(&format!("language: {}\n", q(&self.language)));
        if !self.game.is_empty() {
            out.push_str(&format!("game: {}\n", q(&self.game)));
        }
        if !self.contributors.is_empty() {
            out.push_str("contributors:\n");
            for c in &self.contributors {
                out.push_str(&format!("- {}\n", q(c)));
            }
        }
        if !self.notes.is_empty() {
            out.push_str(&format!("notes: {}\n", q(&self.notes)));
        }
        out.push_str("sections:\n");
        for (name, entries) in self.sections.iter() {
            if entries.is_empty() {
                continue;
            }
            out.push_str(&format!("  {name}:\n"));
            for e in entries {
                out.push_str(&format!("  - key: {}\n", q(&e.key)));
                if !e.context.is_empty() {
                    out.push_str(&format!("    context: {}\n", q(&e.context)));
                }
                // Omitted entirely in a distributable pack (see module docs).
                if !e.source.is_empty() {
                    out.push_str(&format!("    source: {}\n", q(&e.source)));
                }
                out.push_str(&format!("    translation: {}\n", q(&e.translation)));
                out.push_str(&format!("    budget: {}\n", e.budget));
            }
        }
        Ok(out)
    }

    /// Working pack -> **distributable** pack: keep only the entries a
    /// translator filled in, and drop their `source` (and `context`, which
    /// quotes table ids and scene names but can also echo the source text).
    /// The result is a pure `key -> new text` lookup table plus the byte-budget
    /// hint: none of the original script survives.
    pub fn strip_sources(mut self) -> Self {
        for entries in self.sections.each_mut() {
            entries.retain(|e| e.is_filled());
            for e in entries.iter_mut() {
                e.source.clear();
                e.context.clear();
            }
        }
        self
    }

    /// Copy every non-empty `translation` from `other` onto the entry with the
    /// same key here. Powers `translate merge` (chunked skeletons -> one pack)
    /// and `translate init --resume` (a shipped distributable pack -> a fresh
    /// source-bearing working pack, so a translator can keep editing without
    /// anyone redistributing the source text). Returns the number of entries
    /// updated.
    pub fn merge_translations(&mut self, other: &LanguagePack) -> usize {
        let filled: std::collections::HashMap<&str, &str> = other
            .sections
            .iter()
            .flat_map(|(_, e)| e.iter())
            .filter(|e| e.is_filled())
            .map(|e| (e.key.as_str(), e.translation.as_str()))
            .collect();
        let mut n = 0;
        for entries in self.sections.each_mut() {
            for e in entries.iter_mut() {
                if let Some(t) = filled.get(e.key.as_str())
                    && e.translation != *t
                {
                    e.translation = (*t).to_string();
                    n += 1;
                }
            }
        }
        n
    }

    /// Split into chunks of at most `max_entries` entries, preserving section
    /// membership and order. Each chunk is itself a valid pack (same header,
    /// a slice of the entries), so a bulk-fill pass can be parallelized over
    /// the chunks and the results recombined with [`Self::merge_translations`].
    pub fn split_chunks(&self, max_entries: usize) -> Vec<LanguagePack> {
        assert!(max_entries > 0, "chunk size must be positive");
        let mut chunks: Vec<LanguagePack> = Vec::new();
        let mut cur = self.empty_like();
        let mut n = 0usize;
        for (name, entries) in self.sections.iter() {
            for e in entries {
                cur.section_mut(name).push(e.clone());
                n += 1;
                if n == max_entries {
                    chunks.push(std::mem::replace(&mut cur, self.empty_like()));
                    n = 0;
                }
            }
        }
        if n > 0 {
            chunks.push(cur);
        }
        chunks
    }

    /// Same header, no entries.
    fn empty_like(&self) -> LanguagePack {
        LanguagePack {
            sections: Sections::default(),
            ..self.clone()
        }
    }

    /// Section list by serialization name (panics on an unknown name - the set
    /// is closed and comes from [`Sections::iter`]).
    fn section_mut(&mut self, name: &str) -> &mut Vec<Entry> {
        match name {
            "items" => &mut self.sections.items,
            "item_types" => &mut self.sections.item_types,
            "spells" => &mut self.sections.spells,
            "arts" => &mut self.sections.arts,
            "accessory_passives" => &mut self.sections.accessory_passives,
            "party_names" => &mut self.sections.party_names,
            "scene_dialog" => &mut self.sections.scene_dialog,
            "inline_text" => &mut self.sections.inline_text,
            "ui_menu" => &mut self.sections.ui_menu,
            other => unreachable!("unknown section {other}"),
        }
    }

    /// Re-target this pack for a new language: keeps every key / source /
    /// context / budget, clears all translations, stamps the header.
    pub fn into_skeleton(mut self, language: &str, contributors: Vec<String>) -> Self {
        self.language = language.to_string();
        self.contributors = contributors;
        for entries in self.sections.each_mut() {
            for e in entries.iter_mut() {
                e.translation.clear();
            }
        }
        self
    }

    /// Per-section `(name, translated, total)` coverage rows.
    pub fn coverage(&self) -> Vec<(&'static str, usize, usize)> {
        self.sections
            .iter()
            .map(|(name, entries)| {
                let translated = entries
                    .iter()
                    .filter(|e| !e.translation.trim().is_empty())
                    .count();
                (name, translated, entries.len())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LanguagePack {
        let mut p = LanguagePack::new("en");
        p.sections.items.push(Entry {
            key: "scus:str:0x80012260".into(),
            context: "item 0x79".into(),
            source: "Healing Berry".into(),
            translation: String::new(),
            budget: 13,
        });
        p.sections.scene_dialog.push(Entry {
            key: "man:31:0xe7".into(),
            context: "izumi".into(),
            source: "Clean water flows from".into(),
            translation: "L'eau claire coule".into(),
            budget: 22,
        });
        p
    }

    #[test]
    fn yaml_round_trip() {
        let p = sample();
        let y = p.to_yaml().unwrap();
        let back = LanguagePack::from_yaml(&y).unwrap();
        assert_eq!(p, back);
    }

    /// Game text like `Yes` / `No` / `1000G` must emit quoted so YAML 1.1
    /// parsers (PyYAML) don't coerce it to booleans/numbers.
    #[test]
    fn ambiguous_scalars_stay_strings() {
        let mut p = sample();
        p.sections.items[0].source = "Yes".into();
        p.sections.items[0].translation = "No".into();
        p.sections.scene_dialog[0].source = "It's a 'test'".into();
        let y = p.to_yaml().unwrap();
        assert!(y.contains("source: 'Yes'"), "{y}");
        assert!(y.contains("translation: 'No'"), "{y}");
        assert!(y.contains("'It''s a ''test'''"), "{y}");
        let back = LanguagePack::from_yaml(&y).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn format_gate() {
        let mut p = sample();
        p.format = "bogus".into();
        let y = serde_yaml::to_string(&p).unwrap();
        assert!(LanguagePack::from_yaml(&y).is_err());
    }

    #[test]
    fn skeleton_clears_translations_and_stamps_language() {
        let p = sample().into_skeleton("fr", vec!["someone".into()]);
        assert_eq!(p.language, "fr");
        assert_eq!(p.contributors, vec!["someone".to_string()]);
        assert!(p.sections.scene_dialog[0].translation.is_empty());
        assert_eq!(p.sections.scene_dialog[0].source, "Clean water flows from");
    }

    /// The distributable shape: filled entries only, no `source:` / `context:`
    /// anywhere in the emitted YAML, and it still re-parses as a pack.
    #[test]
    fn strip_sources_drops_the_original_script() {
        let dist = sample().strip_sources();
        assert_eq!(dist.sections.items.len(), 0, "unfilled entry dropped");
        assert_eq!(dist.sections.scene_dialog.len(), 1);
        assert!(dist.sections.scene_dialog[0].source.is_empty());
        let y = dist.to_yaml().unwrap();
        assert!(!y.contains("source:"), "{y}");
        assert!(!y.contains("Clean water"), "{y}");
        assert!(!y.contains("context:"), "{y}");
        assert!(y.contains("translation: 'L''eau claire coule'"), "{y}");
        assert!(y.contains("budget: 22"), "{y}");
        let back = LanguagePack::from_yaml(&y).unwrap();
        assert_eq!(back.sections.scene_dialog[0].key, "man:31:0xe7");
        assert_eq!(back.sections.scene_dialog[0].budget, 22);
    }

    /// The resume path: a shipped (source-less) pack merges back onto a fresh
    /// source-bearing skeleton exported from the user's own disc.
    #[test]
    fn merge_translations_resumes_from_a_distributable_pack() {
        let dist = sample().strip_sources();
        let mut skeleton = sample().into_skeleton("fr", vec![]);
        assert_eq!(skeleton.sections.scene_dialog[0].translation, "");
        assert_eq!(skeleton.merge_translations(&dist), 1);
        assert_eq!(
            skeleton.sections.scene_dialog[0].translation,
            "L'eau claire coule"
        );
        // The skeleton still carries source + budget from the disc export.
        assert_eq!(
            skeleton.sections.scene_dialog[0].source,
            "Clean water flows from"
        );
        // Re-merging is a no-op (nothing changes).
        assert_eq!(skeleton.merge_translations(&dist), 0);
    }

    #[test]
    fn split_chunks_partitions_every_entry() {
        let mut p = sample();
        for i in 0..5 {
            p.sections.inline_text.push(Entry {
                key: format!("raw:9:0x{i:x}"),
                context: String::new(),
                source: "x".into(),
                translation: String::new(),
                budget: 1,
            });
        }
        let total = p.sections.total();
        let chunks = p.split_chunks(3);
        assert_eq!(chunks.len(), total.div_ceil(3));
        assert_eq!(
            chunks.iter().map(|c| c.sections.total()).sum::<usize>(),
            total
        );
        // Every key survives exactly once, and the header rides along.
        let keys: Vec<String> = chunks
            .iter()
            .flat_map(|c| c.sections.iter().flat_map(|(_, e)| e.iter()))
            .map(|e| e.key.clone())
            .collect();
        assert_eq!(keys.len(), total);
        assert_eq!(chunks[0].language, p.language);
    }

    #[test]
    fn coverage_counts() {
        let cov = sample().coverage();
        let items = cov.iter().find(|(n, _, _)| *n == "items").unwrap();
        assert_eq!((items.1, items.2), (0, 1));
        let dlg = cov.iter().find(|(n, _, _)| *n == "scene_dialog").unwrap();
        assert_eq!((dlg.1, dlg.2), (1, 1));
    }
}
