//! Import: apply a filled language pack to a disc image.
//!
//! Only entries whose `translation` is non-empty are touched; everything
//! else stays byte-identical. Every write is same-size in place:
//!
//! - `scus:str:*` / `scus:party:*` - encoded bytes + NUL terminator written
//!   over the original string (budget = the original's span);
//! - `man:*` - the segment inside the decompressed scene MAN is overwritten
//!   and space-padded (`0x20`) to its exact original length (the pager walks
//!   segments byte-by-byte, so the framing must not move), then the whole
//!   MAN is recompressed and must fit its original compressed footprint;
//! - `raw:*` - same space-padded overwrite, directly in the PROT entry.
//!
//! Before writing, each target is verified:
//!
//! - a **working pack** (one that carries `source:`) is checked against it -
//!   if the disc bytes already equal the translation the entry counts as
//!   already applied (idempotent re-import); if they match neither, the entry
//!   is skipped with a warning (wrong disc / conflicting patch);
//! - a **distributable pack** (translation-only - see [`super::pack`]) has no
//!   source to check against, so the target is measured *on the disc being
//!   patched*: the string's own span / the segment's own `0x1F .. 0x00`
//!   framing is the byte budget, and the pack's `budget` hint must agree with
//!   it. A disagreement means the pack wasn't built for this image and the
//!   entry is skipped rather than written blind.
//!
//! Encode failures (non-Latin characters, over-budget text) are reported per
//! entry with per-character positions and leave the disc untouched.

use std::collections::BTreeMap;

use anyhow::{Context, Result};

use legaia_asset::man_edit::{self, TextEdit};
use legaia_asset::{item_names, new_game, scene_asset_table};

use crate::disc::DiscPatcher;

use super::export::SceneManText;
use super::markup::{self, Target};
use super::pack::{Entry, LanguagePack};
use super::segments;
use super::ui;

/// Import outcome counters + per-entry diagnostics.
#[derive(Debug, Default)]
pub struct ImportReport {
    /// Entries written to the image.
    pub applied: usize,
    /// Entries whose translation was already on the disc (no write).
    pub already_applied: usize,
    /// Entries with an empty translation (left vanilla).
    pub untranslated: usize,
    /// Per-entry problems: `(key, message)`. Errors never abort the whole
    /// import - the entry is skipped and the rest proceeds.
    pub issues: Vec<(String, String)>,
    /// Keys of the entries counted in [`Self::applied`].
    pub applied_keys: Vec<String>,
    /// Keys of the entries counted in [`Self::already_applied`].
    pub already_keys: Vec<String>,
}

/// Per-section outcome row (see [`ImportReport::section_counts`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionCounts {
    /// Section name (the pack serialization name, e.g. `scene_dialog`).
    pub name: &'static str,
    /// Entries in the pack's section.
    pub total: usize,
    /// Entries a translator filled in (the ones import acts on).
    pub filled: usize,
    /// Filled entries written to the image.
    pub applied: usize,
    /// Filled entries whose translation was already on the disc.
    pub already_applied: usize,
    /// Filled entries skipped with a diagnostic (see the issues list).
    pub skipped: usize,
}

impl ImportReport {
    fn issue(&mut self, key: &str, msg: impl Into<String>) {
        self.issues.push((key.to_string(), msg.into()));
    }

    /// Absorb another report's counters + diagnostics (used to combine the
    /// two-phase dialog / name imports into one user-facing report).
    pub fn merge(&mut self, other: ImportReport) {
        self.applied += other.applied;
        self.already_applied += other.already_applied;
        self.untranslated += other.untranslated;
        self.issues.extend(other.issues);
        self.applied_keys.extend(other.applied_keys);
        self.already_keys.extend(other.already_keys);
    }

    /// Fold this report against the pack it came from into per-section
    /// applied / already-applied / skipped counts. `skipped` counts filled
    /// entries that produced a diagnostic; a filled entry the report never
    /// saw (e.g. a phase import that excluded its section) counts in none of
    /// the outcome columns.
    pub fn section_counts(&self, pack: &LanguagePack) -> Vec<SectionCounts> {
        use std::collections::HashSet;
        let applied: HashSet<&str> = self.applied_keys.iter().map(String::as_str).collect();
        let already: HashSet<&str> = self.already_keys.iter().map(String::as_str).collect();
        let skipped: HashSet<&str> = self.issues.iter().map(|(k, _)| k.as_str()).collect();
        pack.sections
            .iter()
            .map(|(name, entries)| {
                let mut row = SectionCounts {
                    name,
                    total: entries.len(),
                    ..Default::default()
                };
                for e in entries {
                    if !e.is_filled() {
                        continue;
                    }
                    row.filled += 1;
                    if applied.contains(e.key.as_str()) {
                        row.applied += 1;
                    } else if already.contains(e.key.as_str()) {
                        row.already_applied += 1;
                    } else if skipped.contains(e.key.as_str()) {
                        row.skipped += 1;
                    }
                }
                row
            })
            .collect()
    }
}

/// Which key population an import pass touches. The site patcher splits a
/// pack in two: dialog first (its `man:` offsets predate any record
/// relocation by the door / starting-bag randomizers), SCUS name tables last
/// (so randomizer passes that classify items by their **English** names -
/// the equipment-drop gear pool - still see the retail names). See
/// `docs/tooling/translation.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportPhase {
    /// Every entry (the CLI single-shot import).
    All,
    /// Only `man:` / `raw:` dialog-segment entries.
    DialogOnly,
    /// Only `scus:` string / party-name / `ui:` overlay entries. Overlay UI
    /// strings ride with the names phase: nothing in the randomizer relocates
    /// or classifies an overlay string, so writing them last is always safe.
    NamesOnly,
}

/// Parsed provenance key.
enum Key {
    ScusStr { va: u32 },
    ScusParty { slot: usize },
    Man { entry: usize, off: usize },
    Raw { entry: usize, off: usize },
    Ui { prot: usize, va: u32 },
}

fn parse_key(key: &str) -> Option<Key> {
    let mut it = key.split(':');
    match it.next()? {
        "scus" => match it.next()? {
            "str" => {
                let va = it.next()?.strip_prefix("0x")?;
                Some(Key::ScusStr {
                    va: u32::from_str_radix(va, 16).ok()?,
                })
            }
            "party" => Some(Key::ScusParty {
                slot: it.next()?.parse().ok()?,
            }),
            _ => None,
        },
        kind @ ("man" | "raw") => {
            let entry = it.next()?.parse().ok()?;
            let off = usize::from_str_radix(it.next()?.strip_prefix("0x")?, 16).ok()?;
            Some(if kind == "man" {
                Key::Man { entry, off }
            } else {
                Key::Raw { entry, off }
            })
        }
        "ui" => {
            let prot = it.next()?.parse().ok()?;
            let va = u32::from_str_radix(it.next()?.strip_prefix("0x")?, 16).ok()?;
            Some(Key::Ui { prot, va })
        }
        _ => None,
    }
}

/// Encode the pack's `source` for `target`. `Ok(None)` = the pack is a
/// distributable (translation-only) one and carries no source; `Err(())` = a
/// diagnostic was recorded.
#[allow(clippy::result_unit_err)]
fn encode_source(
    entry: &Entry,
    target: Target,
    report: &mut ImportReport,
) -> Result<Option<Vec<u8>>, ()> {
    if entry.source.is_empty() {
        return Ok(None);
    }
    match markup::encode(&entry.source, target) {
        Ok(b) => Ok(Some(b)),
        Err(issues) => {
            report.issue(
                &entry.key,
                format!(
                    "pack source doesn't encode (corrupted pack?): {}",
                    issues[0]
                ),
            );
            Err(())
        }
    }
}

/// Encode `entry.translation` for `target`. `None` = a diagnostic was recorded.
fn encode_translation(entry: &Entry, target: Target, report: &mut ImportReport) -> Option<Vec<u8>> {
    match markup::encode(&entry.translation, target) {
        Ok(b) => Some(b),
        Err(issues) => {
            let detail: Vec<String> = issues.iter().map(ToString::to_string).collect();
            report.issue(
                &entry.key,
                format!("translation not encodable: {}", detail.join("; ")),
            );
            None
        }
    }
}

/// Budget check against the byte span actually measured on the disc.
fn fits(entry: &Entry, translated: &[u8], budget: usize, report: &mut ImportReport) -> bool {
    if translated.len() > budget {
        report.issue(
            &entry.key,
            format!(
                "translation needs {} bytes but the in-place budget is {budget} \
                 (shorten the text)",
                translated.len()
            ),
        );
        return false;
    }
    true
}

/// Wrong-disc guard for a source-less entry: the target's real length on the
/// disc must equal the pack's `budget` hint, which was measured off the disc
/// the pack was authored against. (A working pack proves the same thing, more
/// strongly, by comparing the bytes.)
fn hint_agrees(entry: &Entry, disc_len: usize, report: &mut ImportReport) -> bool {
    if entry.budget != disc_len {
        report.issue(
            &entry.key,
            format!(
                "this disc's text is {disc_len} bytes but the pack expects {} - the pack \
                 was not built for this image (or another patch already moved the text) \
                 - skipped",
                entry.budget
            ),
        );
        return false;
    }
    true
}

/// `translated`, space-padded to exactly `len` bytes (dialog-segment form).
fn pad_segment(translated: &[u8], len: usize) -> Vec<u8> {
    let mut v = translated.to_vec();
    v.resize(len, 0x20);
    v
}

/// Longest SCUS string the reader will follow before calling the pointer bogus.
const MAX_SCUS_STRLEN: usize = 512;

/// Text bytes writable at `off` on **this disc**: the string's own length plus
/// the zero alignment padding that follows its terminator (the name pools are
/// 4-byte aligned; see `export::ScusCollector::padding_slack`). Measured, not
/// asserted - the run must actually be zeros - so a pack can never talk the
/// importer into writing over a neighbouring string.
fn scus_writable_span(scus: &[u8], off: usize, cur_len: usize) -> usize {
    let end = off + cur_len; // the NUL
    let aligned = (end + 4) & !3;
    let mut u = end + 1;
    while u < aligned && scus.get(u) == Some(&0) {
        u += 1;
    }
    (u - 1) - off
}

/// Plan one SCUS-string write: `(file_offset, bytes)`, or `None` when the
/// entry was resolved without a write (diagnostic / already applied).
fn plan_scus_str(
    scus: &[u8],
    entry: &Entry,
    va: u32,
    report: &mut ImportReport,
) -> Option<(usize, Vec<u8>)> {
    let source = encode_source(entry, Target::CString, report).ok()?;
    let translated = encode_translation(entry, Target::CString, report)?;
    let Some(off) = item_names::file_offset_for_va(scus, va) else {
        report.issue(&entry.key, "VA not in the SCUS data segment");
        return None;
    };
    // The string as it stands on this disc, up to (not including) its NUL.
    let Some(tail) = scus.get(off..) else {
        report.issue(&entry.key, "string span past end of SCUS");
        return None;
    };
    let Some(cur_len) = tail
        .iter()
        .take(MAX_SCUS_STRLEN)
        .position(|&b| b == 0)
        .filter(|&l| l > 0)
    else {
        report.issue(&entry.key, "no NUL-terminated string at this VA - skipped");
        return None;
    };
    let cur = &tail[..cur_len];
    if cur == translated.as_slice() {
        report.already_applied += 1;
        report.already_keys.push(entry.key.clone());
        return None;
    }
    // The write may never leave the string's own dead span on THIS disc: its
    // bytes plus the zero padding after its terminator. The pack's budget is
    // clamped to that, so a bad/tampered budget can't reach a neighbour.
    let writable = scus_writable_span(scus, off, cur_len);
    if let Some(src) = &source
        && cur != src.as_slice()
    {
        report.issue(
            &entry.key,
            "disc bytes don't match the pack source (different disc revision or \
             a conflicting patch) - skipped",
        );
        return None;
    }
    // Source-less pack: the measured span is the only wrong-disc guard there is.
    if source.is_none() && !hint_agrees(entry, writable, report) {
        return None;
    }
    if !fits(entry, &translated, entry.budget.min(writable), report) {
        return None;
    }
    let mut bytes = translated;
    bytes.push(0); // re-terminate; the old tail past the NUL is never read
    Some((off, bytes))
}

/// Plan one party-name write (fixed 10-byte NUL-padded field).
fn plan_scus_party(
    scus: &[u8],
    entry: &Entry,
    slot: usize,
    report: &mut ImportReport,
) -> Option<(usize, Vec<u8>)> {
    let source = encode_source(entry, Target::CString, report).ok()?;
    let translated = encode_translation(entry, Target::CString, report)?;
    if !fits(entry, &translated, entry.budget, report) {
        return None;
    }
    if slot >= new_game::PARTY_RECORDS {
        report.issue(&entry.key, "party slot out of range");
        return None;
    }
    if translated.len() > new_game::NAME_LEN - 1 {
        report.issue(&entry.key, "party name must fit 9 bytes");
        return None;
    }
    let va = new_game::PARTY_TEMPLATE_VA + (slot * new_game::RECORD_STRIDE) as u32 + 16;
    let Some(off) = item_names::file_offset_for_va(scus, va) else {
        report.issue(&entry.key, "party record outside the SCUS data segment");
        return None;
    };
    let Some(field) = scus.get(off..off + new_game::NAME_LEN) else {
        report.issue(&entry.key, "party record past end of SCUS");
        return None;
    };
    let cur_len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    if &field[..cur_len] == translated.as_slice() {
        report.already_applied += 1;
        report.already_keys.push(entry.key.clone());
        return None;
    }
    // Fixed-width field: the write is bounded by the field itself, so a
    // source-less (distributable) pack needs no extra guard here.
    if let Some(src) = &source
        && &field[..cur_len] != src.as_slice()
    {
        report.issue(
            &entry.key,
            "disc bytes don't match the pack source - skipped",
        );
        return None;
    }
    let mut bytes = translated;
    bytes.resize(new_game::NAME_LEN, 0);
    Some((off, bytes))
}

/// Plan one overlay UI-string write into a PROT overlay entry buffer:
/// `(file_offset, bytes)`, or `None` when resolved without a write. Same
/// mechanism as [`plan_scus_str`] (NUL-terminated, span + zero-padding budget,
/// wrong-disc guard) but keyed by VA into the overlay at `va - base_va`. The
/// returned bytes fully cover the old string's span so a shorter translation
/// leaves no stale tail behind for the pool scanner to pick up.
fn plan_ui(
    entry: &[u8],
    base_va: u32,
    e: &Entry,
    va: u32,
    report: &mut ImportReport,
) -> Option<(usize, Vec<u8>)> {
    let source = encode_source(e, Target::CString, report).ok()?;
    let translated = encode_translation(e, Target::CString, report)?;
    let Some(off) = va.checked_sub(base_va).map(|d| d as usize) else {
        report.issue(&e.key, "VA is before the overlay load base");
        return None;
    };
    let Some(tail) = entry.get(off..) else {
        report.issue(&e.key, "VA past end of the overlay entry");
        return None;
    };
    let Some(cur_len) = tail
        .iter()
        .take(MAX_SCUS_STRLEN)
        .position(|&b| b == 0)
        .filter(|&l| l > 0)
    else {
        report.issue(&e.key, "no NUL-terminated string at this VA - skipped");
        return None;
    };
    let cur = &tail[..cur_len];
    if cur == translated.as_slice() {
        report.already_applied += 1;
        report.already_keys.push(e.key.clone());
        return None;
    }
    let writable = ui::writable_span(entry, off, cur_len);
    if let Some(src) = &source
        && cur != src.as_slice()
    {
        report.issue(
            &e.key,
            "disc bytes don't match the pack source (different disc revision or \
             a conflicting patch) - skipped",
        );
        return None;
    }
    if source.is_none() && !hint_agrees(e, writable, report) {
        return None;
    }
    if !fits(e, &translated, e.budget.min(writable), report) {
        return None;
    }
    // Cover the whole old string span (its bytes + terminator) so a shorter
    // translation zero-fills the leftover instead of leaving a printable tail.
    let mut bytes = translated;
    bytes.push(0);
    if bytes.len() < cur_len + 1 {
        bytes.resize(cur_len + 1, 0);
    }
    Some((off, bytes))
}

/// One segment edit in the domain the key addresses (a decompressed MAN, or a
/// raw PROT entry), measured and verified against the bytes actually there.
///
/// The segment's byte budget is its own `0x1F <text> 0x00` framing on this
/// disc - never a number the pack asserts - so a bad pack can't overrun the
/// text pool it edits. `Some(byte_len)` = written; `None` = nothing to write
/// (already applied, or a diagnostic was recorded).
fn apply_segment_edit(
    buf: &mut [u8],
    entry: &Entry,
    off: usize,
    source: Option<&[u8]>,
    translated: &[u8],
    report: &mut ImportReport,
) -> Option<usize> {
    // Framing, read off the disc: the 0x1F lead immediately before the keyed
    // text offset, and the segment's own terminator after it.
    let framed = off > 0
        && buf.get(off - 1) == Some(&0x1F)
        && segments::walk_to_terminator(buf, off).is_some_and(|t| buf[t] == 0x00);
    if !framed {
        report.issue(
            &entry.key,
            "segment framing not found at the keyed offset - skipped",
        );
        return None;
    }
    let term = segments::walk_to_terminator(buf, off).expect("framing checked");
    let len = term - off;

    let padded = pad_segment(translated, len);
    if &buf[off..term] == padded.as_slice() {
        report.already_applied += 1;
        report.already_keys.push(entry.key.clone());
        return None;
    }
    match source {
        Some(src) => {
            if &buf[off..term] != src {
                report.issue(
                    &entry.key,
                    "disc bytes don't match the pack source (different disc revision or \
                     a conflicting patch) - skipped",
                );
                return None;
            }
        }
        None if !hint_agrees(entry, len, report) => return None,
        None => {}
    }
    if !fits(entry, translated, len, report) {
        return None;
    }
    buf[off..term].copy_from_slice(&padded);
    Some(len)
}

/// A dialog segment inside a scene MAN, validated against the disc and ready
/// to write: its decompressed-domain offset, current on-disc byte length (the
/// `0x1F .. 0x00` framing span), and the encoded translated bytes.
struct ReadyMan<'a> {
    off: usize,
    old_len: usize,
    translated: Vec<u8>,
    entry: &'a Entry,
}

/// Outcome of validating one dialog segment against the disc before a write.
enum SegPrep {
    /// Ready to write; `old_len` is its current framing span.
    Ready { old_len: usize },
    /// The translation is already on the disc (counted; no write needed).
    Already,
    /// A diagnostic was recorded; skip.
    Skip,
}

/// Validate a dialog segment against the bytes actually on the disc (framing,
/// already-applied, wrong-disc guard) *without* mutating - the same checks as
/// [`apply_segment_edit`], but returning the current framing span so the caller
/// can choose the same-size or grow path. See [`apply_segment_edit`] for the
/// budget rationale.
fn prepare_segment(
    buf: &[u8],
    entry: &Entry,
    off: usize,
    source: Option<&[u8]>,
    translated: &[u8],
    report: &mut ImportReport,
) -> SegPrep {
    let framed = off > 0
        && buf.get(off - 1) == Some(&0x1F)
        && segments::walk_to_terminator(buf, off).is_some_and(|t| buf[t] == 0x00);
    if !framed {
        report.issue(
            &entry.key,
            "segment framing not found at the keyed offset - skipped",
        );
        return SegPrep::Skip;
    }
    let term = segments::walk_to_terminator(buf, off).expect("framing checked");
    let old_len = term - off;
    let cur = &buf[off..term];
    // Already applied: exact (a grown/shrunk write) or space-padded (same-size).
    if cur == translated || cur == pad_segment(translated, old_len).as_slice() {
        report.already_applied += 1;
        report.already_keys.push(entry.key.clone());
        return SegPrep::Already;
    }
    match source {
        Some(src) if cur != src => {
            report.issue(
                &entry.key,
                "disc bytes don't match the pack source (different disc revision or \
                 a conflicting patch) - skipped",
            );
            SegPrep::Skip
        }
        None if !hint_agrees(entry, old_len, report) => SegPrep::Skip,
        _ => SegPrep::Ready { old_len },
    }
}

/// Attempt the **generalized rewriter** path for one scene MAN: grow/shrink
/// every ready segment to its exact translated bytes, relocate all crossing
/// references ([`man_edit::apply_text_edits`]), verify the rewrite is the same
/// program relocated ([`man_edit::text_edits_preserve_scripts`]), and recompress
/// within the MAN's on-disc footprint. Returns `(recompressed_stream,
/// new_decompressed_size)` on success, or `None` when the growth can't be done
/// safely / won't fit (caller falls back to same-size + abbreviation).
fn try_grow_man(man: &SceneManText, ready: &[ReadyMan]) -> Option<(Vec<u8>, u32)> {
    let edits: Vec<TextEdit> = ready
        .iter()
        .map(|r| TextEdit {
            offset: r.off,
            old_len: r.old_len,
            new_bytes: r.translated.clone(),
        })
        .collect();
    let grown = man_edit::apply_text_edits(&man.decoded, &edits).ok()?;
    if !man_edit::text_edits_preserve_scripts(&man.decoded, &grown) {
        return None;
    }
    // Recompress within the descriptor-boundary footprint (fast greedy parse,
    // then the optimal parse when it just misses - same policy as `repack`).
    let stream = legaia_lzs::compress(&grown);
    let stream = if stream.len() <= man.compressed_budget {
        stream
    } else {
        let opt = legaia_lzs::compress_optimal(&grown);
        if opt.len() <= man.compressed_budget {
            opt
        } else {
            return None;
        }
    };
    Some((stream, grown.len() as u32))
}

/// Apply `pack` to the patcher's image. Untranslated entries are untouched.
pub fn import_pack(patcher: &mut DiscPatcher, pack: &LanguagePack) -> Result<ImportReport> {
    import_pack_phase(patcher, pack, ImportPhase::All)
}

/// [`import_pack`] restricted to one key population (see [`ImportPhase`]).
/// Entries outside the phase are ignored entirely - they appear in none of
/// the report's counters - so running `DialogOnly` then `NamesOnly` and
/// [`ImportReport::merge`]-ing the two reports counts every entry exactly
/// once, identically to a single `All` run.
pub fn import_pack_phase(
    patcher: &mut DiscPatcher,
    pack: &LanguagePack,
    phase: ImportPhase,
) -> Result<ImportReport> {
    let mut report = ImportReport::default();

    // Group the work by write mechanism.
    let mut scus_work: Vec<&Entry> = Vec::new();
    let mut man_work: BTreeMap<usize, Vec<(usize, &Entry)>> = BTreeMap::new();
    let mut raw_work: BTreeMap<usize, Vec<(usize, &Entry)>> = BTreeMap::new();
    let mut ui_work: BTreeMap<usize, Vec<(u32, &Entry)>> = BTreeMap::new();
    for (_, entries) in pack.sections.iter() {
        for e in entries {
            let key = parse_key(&e.key);
            let in_phase = match (&key, phase) {
                (_, ImportPhase::All) => true,
                (Some(Key::Man { .. }) | Some(Key::Raw { .. }), ImportPhase::DialogOnly) => true,
                (
                    Some(Key::ScusStr { .. }) | Some(Key::ScusParty { .. }) | Some(Key::Ui { .. }),
                    ImportPhase::NamesOnly,
                ) => true,
                // Unrecognized keys are diagnosed once, in the names (last)
                // phase, so a dialog+names pair reports them exactly once.
                (None, ImportPhase::NamesOnly) => true,
                _ => false,
            };
            if !in_phase {
                continue;
            }
            if e.translation.trim().is_empty() {
                report.untranslated += 1;
                continue;
            }
            match key {
                Some(Key::ScusStr { .. }) | Some(Key::ScusParty { .. }) => scus_work.push(e),
                Some(Key::Man { entry, off }) => {
                    man_work.entry(entry).or_default().push((off, e));
                }
                Some(Key::Raw { entry, off }) => {
                    raw_work.entry(entry).or_default().push((off, e));
                }
                Some(Key::Ui { prot, va }) => {
                    ui_work.entry(prot).or_default().push((va, e));
                }
                None => report.issue(&e.key, "unrecognized key shape - skipped"),
            }
        }
    }

    // SCUS strings. Read the file once; each write is mirrored into the
    // local copy so later verifications stay coherent with earlier writes.
    if !scus_work.is_empty() {
        let mut scus = patcher
            .read_named_file("SCUS_942.54")
            .context("SCUS_942.54 not found in disc image")?;
        for e in &scus_work {
            let plan = match parse_key(&e.key) {
                Some(Key::ScusStr { va }) => plan_scus_str(&scus, e, va, &mut report),
                Some(Key::ScusParty { slot }) => plan_scus_party(&scus, e, slot, &mut report),
                _ => unreachable!(),
            };
            if let Some((off, bytes)) = plan {
                patcher.patch_named_file("SCUS_942.54", off as u64, &bytes)?;
                scus[off..off + bytes.len()].copy_from_slice(&bytes);
                report.applied += 1;
                report.applied_keys.push(e.key.clone());
            }
        }
    }

    // Scene-bundle MANs: one decompress -> N segment edits -> one repack per
    // PROT entry.
    for (entry_idx, edits) in man_work {
        let entry_bytes = match patcher.read_entry(entry_idx) {
            Ok(b) => b,
            Err(e) => {
                for (_, en) in &edits {
                    report.issue(&en.key, format!("PROT entry unreadable: {e}"));
                }
                continue;
            }
        };
        let Some(mut man) = SceneManText::locate(&entry_bytes) else {
            for (_, en) in &edits {
                report.issue(&en.key, "scene MAN not found in this PROT entry - skipped");
            }
            continue;
        };

        // Validate each segment against the disc once (framing / already-applied
        // / wrong-disc guard), collecting the ready set with its current span.
        let mut ready: Vec<ReadyMan> = Vec::new();
        for (off, en) in &edits {
            let Ok(source) = encode_source(en, Target::Segment, &mut report) else {
                continue;
            };
            let Some(translated) = encode_translation(en, Target::Segment, &mut report) else {
                continue;
            };
            if let SegPrep::Ready { old_len } = prepare_segment(
                &man.decoded,
                en,
                *off,
                source.as_deref(),
                &translated,
                &mut report,
            ) {
                ready.push(ReadyMan {
                    off: *off,
                    old_len,
                    translated,
                    entry: en,
                });
            }
        }
        if ready.is_empty() {
            continue;
        }

        // Escape hatch: if any line overflows its own byte span, try to grow the
        // whole MAN (budget = the MAN's own on-disc footprint, not each string).
        // On success the segment framing moves but every crossing reference is
        // relocated, so the pager still walks it correctly.
        if ready.iter().any(|r| r.translated.len() > r.old_len)
            && let Some((stream, new_size)) = try_grow_man(&man, &ready)
        {
            patcher.patch_prot_entry(entry_idx, man.man_offset as u64, &stream)?;
            patcher.patch_prot_entry(
                entry_idx,
                man.man_descriptor_off as u64,
                &scene_asset_table::encode_size_word(0x03, new_size).to_le_bytes(),
            )?;
            report.applied += ready.len();
            report
                .applied_keys
                .extend(ready.iter().map(|r| r.entry.key.clone()));
            continue;
        }

        // Same-size (fast, byte-identical) path: apply the fitting lines in
        // place, report the over-budget ones (the MAN couldn't be grown to fit
        // them), then recompress with a longest-first rollback if the scene's
        // dialog no longer fits its compressed footprint.
        let mut applied: Vec<(usize, Vec<u8>, &Entry)> = Vec::new();
        for r in &ready {
            if r.translated.len() > r.old_len {
                report.issue(
                    &r.entry.key,
                    format!(
                        "translation needs {} bytes but the in-place budget is {} and the \
                         scene MAN could not be grown to fit it (shorten this line)",
                        r.translated.len(),
                        r.old_len
                    ),
                );
                continue;
            }
            let before = man.decoded[r.off..r.off + r.old_len].to_vec();
            let padded = pad_segment(&r.translated, r.old_len);
            man.decoded[r.off..r.off + r.old_len].copy_from_slice(&padded);
            applied.push((r.off, before, r.entry));
        }
        if applied.is_empty() {
            continue;
        }

        // The MAN must recompress into its original footprint. Translated text
        // is less repetitive than the source, so a scene can overflow by a few
        // bytes; roll back the costliest lines (longest first) one at a time
        // rather than losing the whole scene's dialog. `pop()` takes from the
        // vector's tail, so an ASCENDING sort puts the longest line there.
        let mut stream = man.repack();
        if stream.is_none() {
            applied.sort_by_key(|(_, before, _)| before.len());
            while stream.is_none()
                && let Some((off, before, en)) = applied.pop()
            {
                man.decoded[off..off + before.len()].copy_from_slice(&before);
                report.issue(
                    &en.key,
                    format!(
                        "scene {entry_idx}: rolled back - the scene's dialog no longer \
                         recompresses into its {} byte footprint (shorten this line)",
                        man.compressed_budget
                    ),
                );
                if applied.is_empty() {
                    break;
                }
                stream = man.repack();
            }
        }
        match stream {
            Some(stream) if !applied.is_empty() => {
                patcher.patch_prot_entry(entry_idx, man.man_offset as u64, &stream)?;
                report.applied += applied.len();
                report
                    .applied_keys
                    .extend(applied.iter().map(|(_, _, en)| en.key.clone()));
            }
            _ => {}
        }
    }

    // Raw carriers: direct same-size in-place writes, one read per PROT entry.
    for (entry_idx, edits) in raw_work {
        let mut window = match patcher.read_entry(entry_idx) {
            Ok(b) => b,
            Err(e) => {
                for (_, en) in &edits {
                    report.issue(&en.key, format!("PROT entry unreadable: {e}"));
                }
                continue;
            }
        };
        // Carrier gate: the `0x1F <text> 0x00` framing occurs by coincidence
        // throughout binary asset banks, so refuse to write into any entry that
        // isn't a genuine, prose-dense dialog carrier on the disc being patched
        // - writing a "translation" over a coincidental hit corrupts the asset
        // and freezes the game. Real event-script / dungeon-MAN scenes clear
        // the bar with a wide margin (see [`segments::is_dialog_carrier`]).
        if !segments::is_dialog_carrier(&window) {
            for (_, en) in &edits {
                report.issue(
                    &en.key,
                    format!(
                        "PROT entry {entry_idx} is not a dialog carrier on this disc \
                         (binary asset bank - writing here would corrupt it) - skipped"
                    ),
                );
            }
            continue;
        }
        for (off, en) in edits {
            let Ok(source) = encode_source(en, Target::Segment, &mut report) else {
                continue;
            };
            let Some(translated) = encode_translation(en, Target::Segment, &mut report) else {
                continue;
            };
            if let Some(len) = apply_segment_edit(
                &mut window,
                en,
                off,
                source.as_deref(),
                &translated,
                &mut report,
            ) {
                patcher.patch_prot_entry(entry_idx, off as u64, &window[off..off + len])?;
                report.applied += 1;
                report.applied_keys.push(en.key.clone());
            }
        }
    }

    // Overlay UI menu strings: same-size NUL-terminated writes into the menu /
    // battle overlay PROT entries. One read per overlay entry; each write is
    // mirrored into the local copy so later verifications stay coherent.
    for (prot, edits) in ui_work {
        let Some(base_va) = ui::overlay_base_va(prot) else {
            for (_, en) in &edits {
                report.issue(&en.key, "PROT entry is not a mapped UI overlay - skipped");
            }
            continue;
        };
        let mut buf = match patcher.read_entry(prot) {
            Ok(b) => b,
            Err(e) => {
                for (_, en) in &edits {
                    report.issue(&en.key, format!("PROT entry unreadable: {e}"));
                }
                continue;
            }
        };
        for (va, en) in edits {
            if let Some((off, bytes)) = plan_ui(&buf, base_va, en, va, &mut report) {
                patcher.patch_prot_entry(prot, off as u64, &bytes)?;
                buf[off..off + bytes.len()].copy_from_slice(&bytes);
                report.applied += 1;
                report.applied_keys.push(en.key.clone());
            }
        }
    }

    Ok(report)
}
