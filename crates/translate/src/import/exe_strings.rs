//! Strings that live in code images: planning the SCUS table, party-name,
//! place-cell and UI-pool edits, recording the code moves they need, and
//! applying the staged SCUS work.

use super::super::code_strings::{self, CodeStrings};
use super::super::markup::Target;
use super::super::name_pool::NamePool;
use super::super::pack::Entry;
use super::super::ui;
use super::report::{ImportReport, IssueKind};
use super::{Key, encode_source, encode_translation, fits, hint_agrees, parse_key};
use legaia_disc_patch::disc::DiscPatcher;
use legaia_game_tables::{item_names, new_game, worldmap_menu};
use std::collections::BTreeMap;

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

/// What [`plan_scus_str`] decided for one name.
pub(super) enum ScusStrPlan {
    /// Same-size in place: `(file_offset, bytes)`.
    Write(usize, Vec<u8>),
    /// Longer than its span but movable: relocate these bytes (no terminator).
    Grow(Vec<u8>),
}

/// Plan one SCUS-string write, or `None` when the entry was resolved without a
/// write (diagnostic / already applied). A translation over its in-place
/// budget comes back as [`ScusStrPlan::Grow`] when `pool` can move the string
/// (see [`super::super::name_pool`]); otherwise it is diagnosed here.
fn plan_scus_str(
    scus: &[u8],
    entry: &Entry,
    va: u32,
    pool: &NamePool,
    sys: Option<&CodeStrings>,
    report: &mut ImportReport,
) -> Option<ScusStrPlan> {
    let source = encode_source(entry, Target::CString, report).ok()?;
    let translated = encode_translation(entry, Target::CString, report)?;
    let Some(off) = item_names::file_offset_for_va(scus, va) else {
        report.issue(&entry.key, "VA not in the SCUS data segment");
        return None;
    };
    // The string as it stands on this disc, up to (not including) its NUL.
    // A strict system-text pool reads token-aware: `{c1:00}` carries a `0x00`
    // argument that is not the terminator.
    let Some(tail) = scus.get(off..) else {
        report.issue(&entry.key, "string span past end of SCUS");
        return None;
    };
    let strict = ui::pool_for(usize::MAX, va).is_some_and(|p| p.strict);
    let Some(cur_len) = ui::pool_strlen(scus, off, strict).filter(|&l| l <= MAX_SCUS_STRLEN) else {
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
    report.room(&entry.key, entry.budget.min(writable));
    if let Some(src) = &source
        && cur != src.as_slice()
    {
        report.issue_as(
            &entry.key,
            IssueKind::Mismatch,
            "disc bytes don't match the pack source (different disc revision or \
             a conflicting patch) - skipped",
        );
        return None;
    }
    // Source-less pack: the measured span is the only wrong-disc guard there is.
    if source.is_none() && !hint_agrees(entry, writable, report) {
        return None;
    }
    if translated.len() > entry.budget.min(writable)
        && (pool.is_movable(va) || sys.is_some_and(|s| s.is_movable(va)))
    {
        return Some(ScusStrPlan::Grow(translated));
    }
    if !fits(entry, &translated, entry.budget.min(writable), report) {
        return None;
    }
    // Re-terminate and zero the rest of the old string's span: nothing reads
    // past a terminator, but a stale printable tail is what a pool scanner
    // (and any reader that skips zero padding to the next string - the combo
    // string that follows an arts description) would pick up.
    let mut bytes = translated;
    bytes.push(0);
    if bytes.len() < cur_len + 1 {
        bytes.resize(cur_len + 1, 0);
    }
    Some(ScusStrPlan::Write(off, bytes))
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
    report.room(&entry.key, entry.budget.min(new_game::NAME_LEN - 1));
    if !fits(entry, &translated, entry.budget, report) {
        return None;
    }
    if slot >= new_game::PARTY_RECORDS {
        report.issue(&entry.key, "party slot out of range");
        return None;
    }
    if translated.len() > new_game::NAME_LEN - 1 {
        report.issue_as(
            &entry.key,
            IssueKind::OverBudget,
            "party name must fit 9 bytes",
        );
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
        report.issue_as(
            &entry.key,
            IssueKind::Mismatch,
            "disc bytes don't match the pack source - skipped",
        );
        return None;
    }
    let mut bytes = translated;
    bytes.resize(new_game::NAME_LEN, 0);
    Some((off, bytes))
}

/// Plan one fixed-cell write: a `0x20`-byte NUL-padded `SCUS_942.54` field
/// (the world-map quick-travel place names, `legaia_game_tables::worldmap_menu`).
/// The write is bounded by the cell itself, like a party-name field.
fn plan_scus_cell(
    scus: &[u8],
    entry: &Entry,
    va: u32,
    report: &mut ImportReport,
) -> Option<(usize, Vec<u8>)> {
    const CELL: usize = worldmap_menu::NAME_STRIDE;
    let source = encode_source(entry, Target::CString, report).ok()?;
    let translated = encode_translation(entry, Target::CString, report)?;
    report.room(&entry.key, entry.budget.min(CELL - 1));
    if !fits(entry, &translated, entry.budget.min(CELL - 1), report) {
        return None;
    }
    let table_end = worldmap_menu::NAME_TABLE_ADDR + (worldmap_menu::NAME_COUNT * CELL) as u32;
    if va < worldmap_menu::NAME_TABLE_ADDR
        || va >= table_end
        || !((va - worldmap_menu::NAME_TABLE_ADDR) as usize).is_multiple_of(CELL)
    {
        report.issue(&entry.key, "not a place-name cell address - skipped");
        return None;
    }
    let Some(off) = item_names::file_offset_for_va(scus, va) else {
        report.issue(&entry.key, "cell outside the SCUS data segment");
        return None;
    };
    let Some(field) = scus.get(off..off + CELL) else {
        report.issue(&entry.key, "cell past end of SCUS");
        return None;
    };
    let cur_len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    if &field[..cur_len] == translated.as_slice() {
        report.already_applied += 1;
        report.already_keys.push(entry.key.clone());
        return None;
    }
    if let Some(src) = &source
        && &field[..cur_len] != src.as_slice()
    {
        report.issue_as(
            &entry.key,
            IssueKind::Mismatch,
            "disc bytes don't match the pack source - skipped",
        );
        return None;
    }
    let mut bytes = translated;
    bytes.resize(CELL, 0);
    Some((off, bytes))
}

/// Plan one overlay UI-string write into a PROT overlay entry buffer:
/// `(file_offset, bytes)`, or `None` when resolved without a write. Same
/// mechanism as [`plan_scus_str`] (NUL-terminated, span + zero-padding budget,
/// wrong-disc guard) but keyed by VA into the overlay at `va - base_va`. The
/// returned bytes fully cover the old string's span so a shorter translation
/// leaves no stale tail behind for the pool scanner to pick up.
#[allow(clippy::too_many_arguments)]
pub(super) fn plan_ui(
    prot: usize,
    entry: &[u8],
    base_va: u32,
    e: &Entry,
    va: u32,
    pool_strict: bool,
    movable: bool,
    report: &mut ImportReport,
) -> Option<ScusStrPlan> {
    let source = encode_source(e, Target::CString, report).ok()?;
    let translated = encode_translation(e, Target::CString, report)?;
    if let Some(why) = ui::patched_operand_violation(prot, va, &translated) {
        report.issue_as(&e.key, IssueKind::Refused, why);
        return None;
    }
    let Some(off) = va.checked_sub(base_va).map(|d| d as usize) else {
        report.issue(&e.key, "VA is before the overlay load base");
        return None;
    };
    let Some(tail) = entry.get(off..) else {
        report.issue(&e.key, "VA past end of the overlay entry");
        return None;
    };
    let strict = pool_strict;
    let Some(cur_len) = ui::pool_strlen(entry, off, strict).filter(|&l| l <= MAX_SCUS_STRLEN)
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
    report.room(&e.key, e.budget.min(writable));
    if let Some(src) = &source
        && cur != src.as_slice()
    {
        report.issue_as(
            &e.key,
            IssueKind::Mismatch,
            "disc bytes don't match the pack source (different disc revision or \
             a conflicting patch) - skipped",
        );
        return None;
    }
    if source.is_none() && !hint_agrees(e, writable, report) {
        return None;
    }
    if translated.len() > e.budget.min(writable) && movable {
        return Some(ScusStrPlan::Grow(translated));
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
    Some(ScusStrPlan::Write(off, bytes))
}

/// Report a [`code_strings`] relocation: moved strings count as applied, the
/// ones without room as `no_free_run`.
pub(super) fn record_code_moves(
    reloc: &code_strings::CodeRelocation,
    image: usize,
    grow: &[(u32, Vec<u8>)],
    entries: &BTreeMap<u32, &Entry>,
    report: &mut ImportReport,
) {
    for &(from, to) in &reloc.moved {
        let e = entries[&from];
        report.applied += 1;
        report.relocated_strings += 1;
        report.applied_keys.push(e.key.clone());
        report.trace.moved.insert(e.key.clone(), (from, to));
    }
    for va in &reloc.no_room {
        let e = entries[va];
        let need = grow
            .iter()
            .find(|(v, _)| v == va)
            .map_or(0, |(_, b)| b.len());
        report.issue_as(
            &e.key,
            IssueKind::NoFreeRun,
            format!(
                "translation needs {need} bytes but the in-place budget is {} and there is \
                 no free run that long this string's references can reach (shorten it, or \
                 other strings in the same image to free room)",
                e.budget
            ),
        );
    }
    report
        .trace
        .code_layouts
        .insert(image, reloc.layout.clone());
}

/// Plan and apply every SCUS write (`scus:str` / `scus:party` / `scus:cell`
/// entries) into `scus`, in place: the same-size writes first, then the names
/// that outgrew their span moved into the pools' free bytes
/// ([`NamePool::relocate`]). Returns the `(file offset, len)` ranges changed,
/// to mirror onto the disc. `pool` must be built from the executable before
/// any write. Import runs this over the disc's executable; the space report's
/// name fitter ([`super::super::space::NameFitter`]) runs it over a copy, so a live
/// editor sees the importer's own decision for every name.
pub(crate) fn apply_scus_work(
    scus: &mut [u8],
    pool: &NamePool,
    sys: Option<&CodeStrings>,
    work: &[&Entry],
    report: &mut ImportReport,
) -> Vec<(usize, usize)> {
    let mut written = Vec::new();
    let mut grow: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut grow_entries: BTreeMap<u32, &Entry> = BTreeMap::new();
    let mut sys_grow: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut sys_entries: BTreeMap<u32, &Entry> = BTreeMap::new();
    for e in work {
        let plan = match parse_key(&e.key) {
            Some(Key::ScusStr { va }) => match plan_scus_str(scus, e, va, pool, sys, report) {
                Some(ScusStrPlan::Grow(bytes)) if !pool.is_movable(va) => {
                    sys_grow.push((va, bytes));
                    sys_entries.insert(va, e);
                    None
                }
                Some(ScusStrPlan::Grow(bytes)) => {
                    grow.push((va, bytes));
                    grow_entries.insert(va, e);
                    None
                }
                Some(ScusStrPlan::Write(off, bytes)) => Some((off, bytes)),
                None => None,
            },
            Some(Key::ScusParty { slot }) => plan_scus_party(scus, e, slot, report),
            Some(Key::ScusCell { va }) => plan_scus_cell(scus, e, va, report),
            _ => {
                report.issue(&e.key, "not a SCUS key - skipped");
                None
            }
        };
        if let Some((off, bytes)) = plan {
            scus[off..off + bytes.len()].copy_from_slice(&bytes);
            written.push((off, bytes.len()));
            report.applied += 1;
            report.applied_keys.push(e.key.clone());
        }
    }
    // Names that outgrew their span: move them into the pools' free bytes
    // (the in-place writes above already freed every shortened tail). With
    // nothing to move this only measures the pools as written - unless a
    // system string needs room, which the compacted pools' free runs give.
    let reloc = pool.relocate_with(scus, &grow, !sys_grow.is_empty());
    written.extend(reloc.written);
    for m in &reloc.moved {
        let e = grow_entries[&m.from];
        report.applied += 1;
        report.relocated_names += 1;
        report.applied_keys.push(e.key.clone());
        report.trace.moved.insert(e.key.clone(), (m.from, m.to));
    }
    for va in reloc.no_room {
        let e = grow_entries[&va];
        let need = grow
            .iter()
            .find(|(v, _)| *v == va)
            .map_or(0, |(_, b)| b.len());
        report.issue_as(
            &e.key,
            IssueKind::NoFreeRun,
            format!(
                "translation needs {need} bytes but the in-place budget is {} and the \
                 name tables have no free run that long to move it into (shorten this \
                 name, or others in the same tables to free room)",
                e.budget
            ),
        );
    }
    // System strings that outgrew their span: move them, with every
    // reference, into their own pools' compaction or the name pools' free
    // runs (zero now: the compaction above just laid them out).
    if let Some(sys) = sys
        && !sys_grow.is_empty()
    {
        let runs: Vec<(u32, u32)> = reloc
            .layout
            .regions
            .iter()
            .filter(|r| r.free > 0)
            .map(|r| (r.end_va - r.free as u32, r.end_va))
            .collect();
        let extra = legaia_disc_patch::space_ledger::zero_spans(scus, ui::SCUS_POOL_BASE_VA, &runs);
        let sreloc = sys.relocate(scus, &sys_grow, &extra);
        written.extend(sreloc.written.iter().copied());
        record_code_moves(&sreloc, usize::MAX, &sys_grow, &sys_entries, report);
    }
    report.trace.name_layout = Some(reloc.layout);
    written
}

/// The `system_text` strings of `scus`, with every overlay the import can
/// read as a foreign image (a string one of them references stays put).
pub(crate) fn system_strings(patcher: &DiscPatcher, scus: &[u8]) -> CodeStrings {
    let overlays: Vec<(Vec<u8>, u32)> = [897usize, 898, 899]
        .iter()
        .filter_map(|&p| Some((patcher.read_entry(p).ok()?, ui::overlay_base_va(p)?)))
        .collect();
    let foreign: Vec<(&[u8], u32)> = overlays
        .iter()
        .map(|(b, base)| (b.as_slice(), *base))
        .collect();
    CodeStrings::build(
        scus,
        ui::SCUS_POOL_BASE_VA,
        code_strings::pools_of(usize::MAX),
        &foreign,
    )
}

/// The `ui_menu` strings of overlay `prot` (its bytes `buf`, loaded at
/// `base_va`), with the executable and the images resident beside it as
/// foreign images.
pub(crate) fn overlay_strings(
    patcher: &DiscPatcher,
    prot: usize,
    buf: &[u8],
    base_va: u32,
) -> CodeStrings {
    let mut others: Vec<(Vec<u8>, u32)> = Vec::new();
    if let Some(scus) = patcher.read_named_file("SCUS_942.54") {
        others.push((scus, ui::SCUS_POOL_BASE_VA));
    }
    for &p in code_strings::co_resident(prot) {
        if let (Ok(b), Some(base)) = (patcher.read_entry(p), ui::overlay_base_va(p)) {
            others.push((b, base));
        }
    }
    let foreign: Vec<(&[u8], u32)> = others
        .iter()
        .map(|(b, base)| (b.as_slice(), *base))
        .collect();
    CodeStrings::build(buf, base_va, code_strings::pools_of(prot), &foreign)
}
