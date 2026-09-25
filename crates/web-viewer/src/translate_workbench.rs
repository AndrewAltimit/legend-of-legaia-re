//! Translation workbench: an in-browser language-pack editor over the user's
//! own disc.
//!
//! [`Workbench`] keeps one parsed disc resident for the whole editing session
//! (the image is handed over once, never per call), together with the disc's
//! own export (the English source and the room every key has), the working
//! pack the page edits, the importer's SCUS name planner
//! ([`NameFitter`]) and the retail dialog font decoded off the disc. Every
//! number it returns is the importer's: rooms and growth paths come from the
//! space report kernel (`legaia_patcher::translation::space`), widths from
//! [`legaia_font::Font::measure`] against [`legaia_font::TEXT_LIMITS`]. No
//! budget rule is re-derived here.
//!
//! Cost model, per the kernel:
//!
//! - per keystroke: [`Workbench::set_translation`] / [`Workbench::check`]
//!   (encode + measure one line);
//! - after a pause in a scene's lines: [`Workbench::scene_fit`] (one scene
//!   through the importer's scene planner);
//! - after a pause in a SCUS name: [`Workbench::names_fit`];
//! - on demand: [`Workbench::space_report`] (a whole dry-run import on a
//!   scratch copy of the image) and [`Workbench::validate`].
//!
//! Everything is computed in this tab: the text returned is the user's own
//! disc text, and nothing is uploaded.

use std::collections::HashMap;

use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

use legaia_font::{Font, MeasureOptions, PenItem, TextLimit, limit_for};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::translation::markup::{self, Target};
use legaia_patcher::translation::space::{
    NameFitter, SpaceOptions, SpaceReport, space_report_with_export, target_for_key,
};
use legaia_patcher::translation::{LanguagePack, export_pack};

fn err(msg: impl AsRef<str>) -> JsValue {
    JsValue::from_str(msg.as_ref())
}

/// Rows the field dialog pager shows per box (`_DAT_801F2740 = 3`).
pub const ROWS_PER_BOX: u8 = 3;

/// Pixel pitch between dialog rows in the box (`0xF`).
const ROW_PITCH: u32 = 15;

/// The on-screen context a key's text is drawn in, as a
/// [`legaia_font::TEXT_LIMITS`] identifier - `None` where no limit is
/// pinned. `row` is the line's row inside its dialog box (dialog sections
/// only): rows after the first sit beside the page-advance hand.
pub fn limit_context(section: &str, row: Option<u8>) -> Option<&'static str> {
    Some(match section {
        "scene_dialog" | "inline_text" => match row {
            Some(r) if r >= 1 => "field_dialog_row_beside_page_hand",
            _ => "field_dialog_row",
        },
        "items" => "item_list_name",
        "spells" => "status_magic_name",
        "arts" => "status_moves_name",
        "party_names" => "party_name",
        "monster_names" => "battle_intro_enemy_label",
        _ => return None,
    })
}

/// `DAT_800740E8` for a section's surface: the field dialog pager's `1`
/// for dialog lines, `0` everywhere else.
fn glyph_pad_for(section: &str, limit: Option<&TextLimit>) -> u32 {
    match limit {
        Some(l) => u32::from(l.glyph_pad),
        None => u32::from(matches!(section, "scene_dialog" | "inline_text")),
    }
}

/// `(prot, offset)` of a `man:` / `raw:` key.
fn dialog_coord(key: &str) -> Option<(&str, usize, usize)> {
    let (kind, rest) = key.split_once(':')?;
    if kind != "man" && kind != "raw" {
        return None;
    }
    let (prot, off) = rest.split_once(':')?;
    let prot: usize = prot.parse().ok()?;
    let off = usize::from_str_radix(off.strip_prefix("0x")?, 16).ok()?;
    Some((kind, prot, off))
}

/// Box / row numbers for a run of dialog lines, in the order given.
///
/// The pager packs **consecutive** lines into one box: the byte after a
/// line's `0x00` terminator being another `0x1F` lead means "same box, next
/// row", up to [`ROWS_PER_BOX`] rows (`docs/formats/mes.md`, multi-segment
/// box packing). A line is `0x1F <text> 0x00`, so the next line of the same
/// box starts its text `len + 2` bytes after this one's. `lines` are
/// `(key, English text length)`; box numbers count up from `first_box`.
pub fn dialog_boxes(lines: &[(&str, usize)], first_box: u32) -> Vec<(u32, u8)> {
    let mut out = Vec::with_capacity(lines.len());
    let mut bx = first_box;
    let mut prev: Option<((&str, usize), usize, u8)> = None;
    for &(key, len) in lines {
        let coord = dialog_coord(key);
        let row = match (prev, coord) {
            (Some(((pk, pprot), pend, prow)), Some((k, p, off)))
                if pk == k && pprot == p && off == pend && prow + 1 < ROWS_PER_BOX =>
            {
                prow + 1
            }
            _ => {
                if !out.is_empty() {
                    bx += 1;
                }
                0
            }
        };
        out.push((bx, row));
        prev = coord.map(|(k, p, off)| ((k, p), off + len + 2, row));
    }
    out
}

/// Static facts about one key, fixed for the session.
#[derive(Debug, Clone)]
struct RowMeta {
    section: &'static str,
    room_kind: &'static str,
    group: Option<String>,
    cap: Option<usize>,
    dialog_box: Option<(u32, u8)>,
    limit: Option<&'static str>,
    english_len: Option<usize>,
}

/// Snake-case name of a room kind (its serde name).
fn room_kind_name(v: &Value) -> &'static str {
    match v.as_str().unwrap_or("") {
        "string_fixed" => "string_fixed",
        "name_movable" => "name_movable",
        "field" => "field",
        "monster" => "monster",
        "dialog_growable" => "dialog_growable",
        "dialog_fixed" => "dialog_fixed",
        _ => "unknown",
    }
}

/// One line's live check.
#[derive(Debug, Clone, PartialEq)]
pub struct LineCheck {
    /// Encoded length, `None` when a character does not encode.
    pub len: Option<usize>,
    /// `(char index, fragment, reason)` per character that does not encode.
    pub errors: Vec<(usize, String, String)>,
    /// Pen advance of the widest line (a lower bound when `unresolved > 0`).
    pub px: Option<u32>,
    /// `0x7C`-separated lines.
    pub lines: usize,
    /// The width context and its limit.
    pub limit: Option<&'static TextLimit>,
    /// Substitution tokens whose text is unknown here.
    pub unresolved: usize,
}

impl LineCheck {
    /// Pixels past the limit (0 when it fits or no limit is pinned).
    pub fn over_px(&self) -> u32 {
        match (self.limit, self.px) {
            (Some(l), Some(px)) => px.saturating_sub(u32::from(l.max_px)),
            _ => 0,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "len": self.len,
            "errors": self.errors.iter().map(|(i, f, r)| json!({"index": i, "char": f, "msg": r})).collect::<Vec<_>>(),
            "px": self.px,
            "lines": self.lines,
            "limit": self.limit.map(|l| l.context),
            "max_px": self.limit.map(|l| l.max_px),
            "over_px": self.over_px(),
            "unresolved": self.unresolved,
        })
    }
}

/// The session state, native-testable (the `#[wasm_bindgen]` wrapper
/// [`Workbench`] only converts at the edge).
pub struct Core {
    patcher: DiscPatcher,
    scus: Vec<u8>,
    english: LanguagePack,
    pack: LanguagePack,
    /// key -> (section index, entry index) in `pack` / `english`.
    index: HashMap<String, (usize, usize)>,
    meta: HashMap<String, RowMeta>,
    disc_report: SpaceReport,
    names: NameFitter,
    font: Option<Font>,
}

impl Core {
    /// Parse the disc, export its text, build the disc-only space report,
    /// the name planner and the font.
    pub fn open(image: Vec<u8>) -> anyhow::Result<Self> {
        let patcher = DiscPatcher::open(image)?;
        let scus = patcher
            .read_named_file("SCUS_942.54")
            .ok_or_else(|| anyhow::anyhow!("SCUS_942.54 not found in disc image"))?;
        let english = export_pack(&patcher)?;
        let disc_report =
            space_report_with_export(&patcher, &english, None, SpaceOptions::default())?;
        let names = NameFitter::from_scus(scus.clone());
        let font = patcher
            .read_prot_bytes(
                legaia_font::FONT_TIM_PROT_DAT_OFFSET,
                legaia_font::FONT_TIM_LEN,
            )
            .ok()
            .and_then(|tim| Font::from_disc_tim_and_scus(&tim, &scus).ok());
        let mut core = Self {
            patcher,
            scus,
            pack: english.clone(),
            english,
            index: HashMap::new(),
            meta: HashMap::new(),
            disc_report,
            names,
            font,
        };
        core.build_meta();
        Ok(core)
    }

    fn build_meta(&mut self) {
        let rows: HashMap<&str, (&'static str, Option<String>, Option<usize>)> = self
            .disc_report
            .entries
            .iter()
            .map(|e| {
                let rk = room_kind_name(&serde_json::to_value(e.room_kind).unwrap_or(Value::Null));
                (e.key.as_str(), (rk, e.group.clone(), e.english_len))
            })
            .collect();
        let caps: HashMap<&str, usize> = self
            .disc_report
            .monsters
            .iter()
            .map(|m| (m.key.as_str(), m.cap))
            .collect();
        let mut index = HashMap::new();
        let mut meta = HashMap::new();
        let mut next_box = 0u32;
        for (si, (section, entries)) in self.english.sections.iter().enumerate() {
            let boxes = if matches!(section, "scene_dialog" | "inline_text") {
                let lines: Vec<(&str, usize)> =
                    entries.iter().map(|e| (e.key.as_str(), e.budget)).collect();
                let b = dialog_boxes(&lines, next_box);
                next_box = b.last().map_or(next_box, |&(n, _)| n + 1);
                Some(b)
            } else {
                None
            };
            for (ei, e) in entries.iter().enumerate() {
                index.insert(e.key.clone(), (si, ei));
                let (rk, group, english_len) = rows
                    .get(e.key.as_str())
                    .cloned()
                    .unwrap_or(("unknown", None, None));
                let dialog_box = boxes.as_ref().map(|b| b[ei]);
                meta.insert(
                    e.key.clone(),
                    RowMeta {
                        section,
                        room_kind: rk,
                        group,
                        cap: caps.get(e.key.as_str()).copied(),
                        dialog_box,
                        limit: limit_context(section, dialog_box.map(|(_, r)| r)),
                        english_len,
                    },
                );
            }
        }
        self.index = index;
        self.meta = meta;
    }

    fn entry(&self, key: &str) -> Option<&legaia_patcher::translation::Entry> {
        let &(si, ei) = self.index.get(key)?;
        self.pack
            .sections
            .iter()
            .nth(si)
            .and_then(|(_, es)| es.get(ei))
    }

    fn english_entry(&self, key: &str) -> Option<&legaia_patcher::translation::Entry> {
        let &(si, ei) = self.index.get(key)?;
        self.english
            .sections
            .iter()
            .nth(si)
            .and_then(|(_, es)| es.get(ei))
    }

    /// The text a key shows with this pack: its translation, else English.
    fn shown_text(&self, key: &str) -> Option<&str> {
        let e = self.entry(key)?;
        Some(if e.is_filled() {
            e.translation.as_str()
        } else {
            e.source.as_str()
        })
    }

    fn scus_u32(&self, va: u32) -> Option<u32> {
        let off = legaia_asset::item_names::file_offset_for_va(&self.scus, va)?;
        Some(u32::from_le_bytes(
            self.scus.get(off..off + 4)?.try_into().ok()?,
        ))
    }

    fn scus_cstr(&self, va: u32, max: usize) -> Option<Vec<u8>> {
        let off = legaia_asset::item_names::file_offset_for_va(&self.scus, va)?;
        let s = self.scus.get(off..(off + max).min(self.scus.len()))?;
        Some(s.iter().copied().take_while(|&b| b != 0).collect())
    }

    /// The name string at SCUS `va` as this pack draws it.
    fn name_at(&self, va: u32) -> Option<Vec<u8>> {
        let key = format!("scus:str:0x{va:08x}");
        match self.shown_text(&key) {
            Some(t) => markup::encode(t, Target::CString).ok(),
            None => self.scus_cstr(va, 64),
        }
    }

    /// Resolve a substitution token the way `FUN_80036514` splices it
    /// (`docs/formats/dialog-font.md`), reading names through the pack:
    /// `0xC1` a party name, `0xC2` / `0xC4` an item name, `0xC3` a spell
    /// name, `0xC7` an 8-byte SCUS name. `0xC5` (an arts name matched on
    /// `[character, art]`) and a party token for the active character stay
    /// unresolved.
    fn resolve(&self, op: u8, arg: u8) -> Option<Vec<u8>> {
        match op {
            0xC1 => {
                let t = self.shown_text(&format!("scus:party:{arg}"))?;
                markup::encode(t, Target::CString).ok()
            }
            0xC2 | 0xC4 => {
                let va = self.scus_u32(
                    legaia_asset::item_names::TABLE_VA
                        + u32::from(arg) * legaia_asset::item_names::RECORD_STRIDE as u32,
                )?;
                self.name_at(va)
            }
            0xC3 => {
                let va = self.scus_u32(
                    legaia_asset::spell_names::STATS_VA
                        + u32::from(arg) * legaia_asset::spell_names::RECORD_STRIDE as u32
                        + 8,
                )?;
                self.name_at(va)
            }
            0xC7 => self.scus_cstr(0x8007_3F24 + u32::from(arg) * 8, 8),
            _ => None,
        }
    }

    /// Encode + measure `text` as key `key`'s line.
    pub fn check(&self, key: &str, text: &str) -> LineCheck {
        let meta = self.meta.get(key);
        let section = meta.map_or("", |m| m.section);
        let limit = meta.and_then(|m| m.limit).and_then(limit_for);
        let target = target_for_key(key);
        let (bytes, errors) = match markup::encode(text, target) {
            Ok(b) => (Some(b), Vec::new()),
            Err(issues) => (
                None,
                issues
                    .into_iter()
                    .map(|i| (i.position, i.fragment, i.reason))
                    .collect(),
            ),
        };
        let mut check = LineCheck {
            len: bytes.as_ref().map(Vec::len),
            errors,
            px: None,
            lines: 1,
            limit,
            unresolved: 0,
        };
        if let (Some(bytes), Some(font)) = (bytes, self.font.as_ref()) {
            let expand = |op: u8, arg: u8| self.resolve(op, arg);
            let opts = MeasureOptions {
                glyph_pad: glyph_pad_for(section, limit),
                expand: Some(&expand),
                ..MeasureOptions::default()
            };
            let m = font.measure(&bytes, &opts);
            check.px = Some(m.max_px);
            check.lines = m.lines();
            check.unresolved = m.unresolved.len();
        }
        check
    }

    /// Set key `key`'s translation (empty = untranslated). `false` for an
    /// unknown key.
    pub fn set_translation(&mut self, key: &str, text: &str) -> bool {
        let Some(&(si, ei)) = self.index.get(key) else {
            return false;
        };
        let secs = self.pack.sections.each_mut();
        match secs.into_iter().nth(si).and_then(|v| v.get_mut(ei)) {
            Some(e) => {
                e.translation = text.to_string();
                true
            }
            None => false,
        }
    }

    /// Start a fresh pack for `language` (every translation empty).
    pub fn start_fresh(&mut self, language: &str) {
        self.pack = self.english.clone().into_skeleton(language, Vec::new());
    }

    /// Load `yaml` (working or shareable) onto this disc's export: header
    /// and every filled translation whose key the disc carries. Returns
    /// `(filled in the file, merged, keys the disc does not carry)`.
    pub fn load_pack(&mut self, yaml: &str) -> anyhow::Result<(usize, usize, usize)> {
        let theirs = LanguagePack::from_yaml(yaml)?;
        let filled: Vec<&str> = theirs
            .sections
            .iter()
            .flat_map(|(_, es)| es)
            .filter(|e| e.is_filled())
            .map(|e| e.key.as_str())
            .collect();
        let unknown = filled
            .iter()
            .filter(|k| !self.index.contains_key(**k))
            .count();
        let mut pack = self
            .english
            .clone()
            .into_skeleton(&theirs.language, theirs.contributors.clone());
        pack.notes = theirs.notes.clone();
        let merged = pack.merge_translations(&theirs);
        self.pack = pack;
        Ok((filled.len(), merged, unknown))
    }

    /// Header fields of the working pack.
    pub fn set_meta(&mut self, language: &str, contributors: Vec<String>, notes: &str) {
        if !language.trim().is_empty() {
            self.pack.language = language.trim().to_string();
        }
        self.pack.contributors = contributors;
        self.pack.notes = notes.to_string();
    }

    /// The working pack (source-bearing: the user's own disc text).
    pub fn working_yaml(&self) -> anyhow::Result<String> {
        self.pack.to_yaml()
    }

    /// The shareable pack: filled keys + translations only.
    pub fn shareable_yaml(&self) -> anyhow::Result<(String, usize)> {
        let dist = self.pack.clone().strip_sources();
        let kept = dist.sections.total();
        Ok((dist.to_yaml()?, kept))
    }

    /// `{language, contributors, notes, t: {key: translation}}` - the
    /// filled translations only, for the page's local autosave.
    pub fn translations_json(&self) -> String {
        let t: serde_json::Map<String, Value> = self
            .pack
            .sections
            .iter()
            .flat_map(|(_, es)| es)
            .filter(|e| e.is_filled())
            .map(|e| (e.key.clone(), Value::String(e.translation.clone())))
            .collect();
        json!({
            "language": self.pack.language,
            "contributors": self.pack.contributors,
            "notes": self.pack.notes,
            "t": t,
        })
        .to_string()
    }

    /// Restore [`Self::translations_json`] output onto a fresh pack.
    /// Returns how many keys were restored.
    pub fn load_translations_json(&mut self, s: &str) -> anyhow::Result<usize> {
        let v: Value = serde_json::from_str(s)?;
        let lang = v["language"].as_str().unwrap_or("xx").to_string();
        self.start_fresh(&lang);
        let contributors = v["contributors"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        self.set_meta(&lang, contributors, v["notes"].as_str().unwrap_or(""));
        let mut n = 0;
        if let Some(t) = v["t"].as_object() {
            for (k, text) in t {
                if let Some(text) = text.as_str()
                    && self.set_translation(k, text)
                {
                    n += 1;
                }
            }
        }
        Ok(n)
    }

    /// Every key as one row: static facts plus the live check of its
    /// current translation.
    pub fn entries_json(&self) -> String {
        let mut rows = Vec::with_capacity(self.index.len());
        for (section, es) in self.pack.sections.iter() {
            for e in es {
                let m = &self.meta[&e.key];
                let c = if e.is_filled() {
                    Some(self.check(&e.key, &e.translation))
                } else {
                    None
                };
                let src = self.english_entry(&e.key).map_or("", |x| x.source.as_str());
                let src_px = self.check(&e.key, src).px;
                rows.push(json!({
                    "k": e.key,
                    "s": section,
                    "c": e.context,
                    "src": src,
                    "t": e.translation,
                    "room": e.budget,
                    "rk": m.room_kind,
                    "g": m.group,
                    "cap": m.cap,
                    "box": m.dialog_box.map(|b| b.0),
                    "row": m.dialog_box.map(|b| b.1),
                    "lim": m.limit,
                    "el": m.english_len,
                    "spx": src_px,
                    "len": c.as_ref().and_then(|c| c.len),
                    "bad": c.as_ref().map_or(0, |c| c.errors.len()),
                    "px": c.as_ref().and_then(|c| c.px),
                    "ovpx": c.as_ref().map_or(0, |c| c.over_px()),
                    "unres": c.as_ref().map_or(0, |c| c.unresolved),
                }));
            }
        }
        json!({
            "language": self.pack.language,
            "contributors": self.pack.contributors,
            "notes": self.pack.notes,
            "limits": legaia_font::TEXT_LIMITS.iter().map(|l| json!({
                "context": l.context, "max_px": l.max_px, "max_lines": l.max_lines,
                "glyph_pad": l.glyph_pad,
            })).collect::<Vec<_>>(),
            "font": self.font.is_some(),
            "entries": rows,
        })
        .to_string()
    }

    /// The disc-only space report without its per-key rows (those ride
    /// [`Self::entries_json`]): name regions, pools, scenes, carriers,
    /// monsters and the summary.
    pub fn disc_report_json(&self) -> String {
        let mut v = serde_json::to_value(&self.disc_report).unwrap_or(Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.remove("entries");
            o.remove("names");
        }
        v.to_string()
    }

    /// The whole dry run for the working pack. Per-key rows are kept for
    /// filled keys only (an untranslated key has no outcome to show);
    /// `reasons` buckets the skips the way the ROM patcher's report does.
    pub fn space_report(&self, relayout: bool) -> anyhow::Result<String> {
        let mut r = space_report_with_export(
            &self.patcher,
            &self.english,
            Some(&self.pack),
            SpaceOptions { relayout },
        )?;
        r.entries.retain(|e| {
            e.outcome
                .is_some_and(|o| o != legaia_patcher::translation::space::Outcome::Untranslated)
        });
        r.names.clear();
        let mut reasons: std::collections::BTreeMap<&'static str, usize> = Default::default();
        for e in &r.entries {
            if let Some(msg) = &e.issue {
                *reasons
                    .entry(crate::rom_patcher::issue_reason(msg))
                    .or_default() += 1;
            }
        }
        let mut v = serde_json::to_value(&r)?;
        if let Some(o) = v.as_object_mut() {
            o.insert(
                "reasons".into(),
                reasons
                    .into_iter()
                    .map(|(reason, count)| json!({"reason": reason, "count": count}))
                    .collect::<Vec<_>>()
                    .into(),
            );
        }
        Ok(v.to_string())
    }

    /// One scene MAN re-planned with the working pack's lines for it.
    pub fn scene_fit(&self, prot: usize, relayout: bool) -> String {
        let fit = legaia_patcher::translation::space::scene_fit(
            &self.patcher,
            &self.pack,
            prot,
            relayout,
        );
        serde_json::to_string(&fit).unwrap_or_else(|_| "null".into())
    }

    /// The importer's SCUS pass over the working pack's names.
    pub fn names_fit(&self) -> String {
        serde_json::to_string(&self.names.fit(&self.pack)).unwrap_or_else(|_| "null".into())
    }

    /// The working pack imported onto a scratch copy of the disc, as the
    /// ROM patcher's check does.
    pub fn validate(
        &self,
        relayout: bool,
    ) -> anyhow::Result<legaia_patcher::translation::ImportReport> {
        let mut scratch = DiscPatcher::open(self.patcher.image().to_vec())?;
        if relayout {
            legaia_patcher::translation::import_pack_relayout(&mut scratch, &self.pack)
        } else {
            legaia_patcher::translation::import_pack(&mut scratch, &self.pack)
        }
    }

    /// Draw `rows` (one string per box row; `|` breaks inside a row) in the
    /// retail font, as key `key`'s surface draws them, at native resolution.
    /// Returns `(w, h, rgba, widest px, unresolved tokens)`.
    pub fn render(&self, key: &str, rows: &[&str]) -> Option<Rendered> {
        let font = self.font.as_ref()?;
        let meta = self.meta.get(key);
        let section = meta.map_or("", |m| m.section);
        let limit = meta.and_then(|m| m.limit).and_then(limit_for);
        let target = target_for_key(key);
        let expand = |op: u8, arg: u8| self.resolve(op, arg);
        let opts = MeasureOptions {
            glyph_pad: glyph_pad_for(section, limit),
            expand: Some(&expand),
            ..MeasureOptions::default()
        };
        let mut placed: Vec<(u32, PenItem)> = Vec::new();
        let mut line = 0u32;
        let mut widest = 0u32;
        let mut unresolved = 0usize;
        for row in rows {
            let bytes = markup::encode(row, target).ok().unwrap_or_default();
            let (items, m) = font.pen_items(&bytes, &opts);
            widest = widest.max(m.max_px);
            unresolved += m.unresolved.len();
            for it in items {
                placed.push((line, it));
            }
            line += m.lines() as u32;
        }
        let lines = line.max(1);
        let limit_px = limit.map_or(0, |l| u32::from(l.max_px));
        let margin = 8u32;
        let w = (widest.max(limit_px) + 2 * margin + 4).min(1024);
        let h = lines * ROW_PITCH + 2 * margin;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let mut put = |x: u32, y: u32, c: [u8; 3]| {
            if x < w && y < h {
                let o = ((y * w + x) * 4) as usize;
                rgba[o..o + 3].copy_from_slice(&c);
                rgba[o + 3] = 255;
            }
        };
        // Window: a dark fill with a light rim.
        for y in 0..h {
            for x in 0..w {
                let rim = x == 0 || y == 0 || x == w - 1 || y == h - 1;
                let past = limit_px > 0 && x >= margin + limit_px && !rim;
                let c = if rim {
                    [0xB8, 0xC0, 0xD8]
                } else if past {
                    [0x4A, 0x18, 0x22]
                } else {
                    [0x10, 0x18, 0x48]
                };
                put(x, y, c);
            }
        }
        if limit_px > 0 {
            for y in 1..h - 1 {
                if y % 4 < 2 {
                    put(margin + limit_px, y, [0xFF, 0x60, 0x60]);
                }
            }
        }
        let (aw, _) = font.atlas_dimensions();
        let atlas = font.atlas_rgba();
        for (base, it) in placed {
            match it {
                PenItem::Glyph { line, x, byte } => {
                    let Some((ox, oy)) = Font::glyph_origin(byte) else {
                        continue;
                    };
                    let py = margin + (base + line) * ROW_PITCH;
                    for gy in 0..legaia_font::GLYPH_H {
                        for gx in 0..legaia_font::GLYPH_W {
                            let a = (((oy + gy) * aw + ox + gx) * 4) as usize;
                            if atlas.get(a + 3).copied().unwrap_or(0) == 0 {
                                continue;
                            }
                            put(
                                margin + x + gx,
                                py + gy,
                                [atlas[a], atlas[a + 1], atlas[a + 2]],
                            );
                        }
                    }
                }
                PenItem::Escape { line, x, width } => {
                    let py = margin + (base + line) * ROW_PITCH;
                    for gx in 0..width.saturating_sub(1) {
                        put(margin + x + gx, py + 2, [0x70, 0x80, 0xA0]);
                        put(margin + x + gx, py + 12, [0x70, 0x80, 0xA0]);
                    }
                }
            }
        }
        Some(Rendered {
            w,
            h,
            rgba,
            widest,
            limit_px,
            unresolved,
        })
    }
}

/// [`Core::render`]'s image.
pub struct Rendered {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    pub widest: u32,
    pub limit_px: u32,
    pub unresolved: usize,
}

/// The workbench session, held by the page for as long as it edits.
#[wasm_bindgen]
pub struct Workbench {
    core: Core,
}

#[wasm_bindgen]
impl Workbench {
    /// Parse the user's disc and prepare the session (export, disc-only
    /// space report, name planner, font). Takes a few seconds.
    pub fn open(image: Vec<u8>) -> Result<Workbench, JsValue> {
        Core::open(image)
            .map(|core| Workbench { core })
            .map_err(|e| err(format!("open disc: {e:#}")))
    }

    /// Every key as a row (see [`Core::entries_json`]).
    pub fn entries(&self) -> String {
        self.core.entries_json()
    }

    /// The disc-only space tables for the dashboard.
    pub fn disc_report(&self) -> String {
        self.core.disc_report_json()
    }

    /// Start an empty pack for `language`.
    pub fn start_fresh(&mut self, language: &str) {
        self.core.start_fresh(language);
    }

    /// Load a pack (working or shareable) onto this disc. Returns
    /// `{language, filled, merged, unknown}` as JSON.
    pub fn load_pack(&mut self, yaml: &str) -> Result<String, JsValue> {
        let (filled, merged, unknown) = self
            .core
            .load_pack(yaml)
            .map_err(|e| err(format!("load pack: {e:#}")))?;
        Ok(json!({
            "language": self.core.pack.language,
            "filled": filled,
            "merged": merged,
            "unknown": unknown,
        })
        .to_string())
    }

    /// Restore the page's autosave. Returns the restored key count.
    pub fn load_translations(&mut self, json: &str) -> Result<u32, JsValue> {
        self.core
            .load_translations_json(json)
            .map(|n| n as u32)
            .map_err(|e| err(format!("restore: {e:#}")))
    }

    /// The filled translations as JSON, for the page's autosave.
    pub fn translations(&self) -> String {
        self.core.translations_json()
    }

    /// Set the pack header.
    pub fn set_meta(&mut self, language: &str, contributors: &str, notes: &str) {
        let c = contributors
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        self.core.set_meta(language, c, notes);
    }

    /// Store one edit and return its live check as JSON
    /// (`{len, errors:[{index,char,msg}], px, lines, limit, max_px, over_px,
    /// unresolved}`), or `null` for an unknown key.
    pub fn set_translation(&mut self, key: &str, text: &str) -> String {
        if !self.core.set_translation(key, text) {
            return "null".into();
        }
        self.core.check(key, text).to_json().to_string()
    }

    /// The live check of `text` as key `key`'s line, without storing it.
    pub fn check(&self, key: &str, text: &str) -> String {
        self.core.check(key, text).to_json().to_string()
    }

    /// Pen width of `text` as key `key`'s surface measures it.
    pub fn measure(&self, key: &str, text: &str) -> Option<u32> {
        self.core.check(key, text).px
    }

    /// Re-plan scene MAN `prot` with the working pack (fast path).
    pub fn scene_fit(&self, prot: u32, relayout: bool) -> String {
        self.core.scene_fit(prot as usize, relayout)
    }

    /// Re-plan every SCUS name of the working pack (fast path): the name
    /// regions' free bytes after the pack and one row per filled name.
    pub fn names_fit(&self) -> String {
        self.core.names_fit()
    }

    /// The full space report for the working pack (a whole dry-run import;
    /// seconds).
    pub fn space_report(&self, relayout: bool) -> Result<String, JsValue> {
        self.core
            .space_report(relayout)
            .map_err(|e| err(format!("space report: {e:#}")))
    }

    /// The ROM patcher's check (`validate_lang_pack`) for the working pack:
    /// the same per-section coverage report.
    pub fn validate(&self, relayout: bool) -> Result<JsValue, JsValue> {
        let report = self
            .core
            .validate(relayout)
            .map_err(|e| err(format!("dry run: {e:#}")))?;
        let sections = report.section_counts(&self.core.pack);
        crate::rom_patcher::lang_report_json(&self.core.pack.language, &report, &sections)
    }

    /// The working pack as YAML (holds the disc's text).
    pub fn working_pack(&self) -> Result<String, JsValue> {
        self.core
            .working_yaml()
            .map_err(|e| err(format!("emit YAML: {e:#}")))
    }

    /// The shareable pack as YAML (no disc text).
    pub fn shareable_pack(&self) -> Result<String, JsValue> {
        self.core
            .shareable_yaml()
            .map(|(y, _)| y)
            .map_err(|e| err(format!("emit YAML: {e:#}")))
    }

    /// A fresh working pack from this disc for `language`, seeded from
    /// `resume` (the ROM patcher's export). Does not touch the session pack.
    pub fn export_pack(&self, language: &str, resume: Option<String>) -> Result<String, JsValue> {
        let mut pack = if language.is_empty() || language == "en" {
            self.core.english.clone()
        } else {
            self.core
                .english
                .clone()
                .into_skeleton(language, Vec::new())
        };
        if let Some(prev) = resume.as_deref().map(str::trim).filter(|y| !y.is_empty()) {
            let seed = LanguagePack::from_yaml(prev)
                .map_err(|e| err(format!("parse resume pack: {e:#}")))?;
            pack.merge_translations(&seed);
        }
        pack.to_yaml().map_err(|e| err(format!("emit YAML: {e:#}")))
    }

    /// Draw `rows` (newline-separated box rows) in the retail font as key
    /// `key`'s surface does. Returns `{w, h, rgba, width_px, limit_px,
    /// unresolved}` or `null` when the disc's font did not decode.
    pub fn render_preview(&self, key: &str, rows: &str) -> Result<JsValue, JsValue> {
        let rows: Vec<&str> = rows.split('\n').collect();
        let Some(r) = self.core.render(key, &rows) else {
            return Ok(JsValue::NULL);
        };
        let o = js_sys::Object::new();
        let set = |k: &str, v: JsValue| js_sys::Reflect::set(&o, &k.into(), &v);
        set("w", JsValue::from_f64(r.w as f64))?;
        set("h", JsValue::from_f64(r.h as f64))?;
        let arr = js_sys::Uint8Array::new_with_length(r.rgba.len() as u32);
        arr.copy_from(&r.rgba);
        set("rgba", arr.into())?;
        set("width_px", JsValue::from_f64(r.widest as f64))?;
        set("limit_px", JsValue::from_f64(r.limit_px as f64))?;
        set("unresolved", JsValue::from_f64(r.unresolved as f64))?;
        Ok(o.into())
    }
}

/// Encoded length of `text` for a key shaped like `key`, or every
/// character that does not encode: `{bytes, errors:[{index, char, msg}]}`.
#[wasm_bindgen]
pub fn encode_check(text: &str, key: &str) -> String {
    match markup::encode(text, target_for_key(key)) {
        Ok(b) => json!({"bytes": b.len(), "errors": []}),
        Err(issues) => json!({
            "bytes": null,
            "errors": issues.iter().map(|i| json!({"index": i.position, "char": i.fragment, "msg": i.reason})).collect::<Vec<_>>(),
        }),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_lines_share_a_box_of_three() {
        // 1F "abc" 00 1F "de" 00 1F "f" 00 1F "g" 00  (text at 1, 6, 10, 13)
        let lines = [
            ("man:5:0x1", 3),
            ("man:5:0x6", 2),
            ("man:5:0xa", 1),
            ("man:5:0xd", 1),
        ];
        let b = dialog_boxes(&lines, 0);
        assert_eq!(b, vec![(0, 0), (0, 1), (0, 2), (1, 0)]);
    }

    #[test]
    fn a_gap_or_another_scene_starts_a_box() {
        let lines = [
            ("man:5:0x1", 3),
            ("man:5:0x20", 2),
            ("man:6:0x24", 2),
            ("raw:6:0x28", 2),
        ];
        let b = dialog_boxes(&lines, 7);
        assert_eq!(b, vec![(7, 0), (8, 0), (9, 0), (10, 0)]);
    }

    #[test]
    fn contexts_map_to_pinned_limits() {
        for (section, row) in [
            ("scene_dialog", Some(0)),
            ("inline_text", Some(2)),
            ("items", None),
            ("spells", None),
            ("arts", None),
            ("party_names", None),
            ("monster_names", None),
        ] {
            let ctx = limit_context(section, row).unwrap();
            assert!(limit_for(ctx).is_some(), "{section}: {ctx}");
        }
        assert_eq!(
            limit_context("scene_dialog", Some(1)),
            Some("field_dialog_row_beside_page_hand")
        );
        assert_eq!(limit_context("ui_menu", None), None);
        assert_eq!(limit_context("system_text", None), None);
    }

    #[test]
    fn dialog_pads_one_pixel_menus_none() {
        assert_eq!(
            glyph_pad_for("scene_dialog", limit_for("field_dialog_row")),
            1
        );
        assert_eq!(glyph_pad_for("items", limit_for("item_list_name")), 0);
        assert_eq!(glyph_pad_for("inline_text", None), 1);
        assert_eq!(glyph_pad_for("ui_menu", None), 0);
    }

    #[test]
    fn encode_check_reports_every_bad_char() {
        let v: Value = serde_json::from_str(&encode_check("abc", "man:1:0x2")).unwrap();
        assert_eq!(v["bytes"], 3);
        let v: Value =
            serde_json::from_str(&encode_check("\u{00e9}x\u{00e9}", "man:1:0x2")).unwrap();
        assert!(v["bytes"].is_null());
        assert_eq!(v["errors"].as_array().unwrap().len(), 2);
    }
}
