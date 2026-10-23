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
//!   MAN is recompressed and must fit its original compressed footprint.
//!   A line longer than its span, or a padded scene that no longer
//!   recompresses, goes through the relocator instead (every line at its own
//!   length, crossing references moved - [`man_edit::apply_text_edits`]),
//!   still inside the same footprint;
//! - `raw:*` - same space-padded overwrite, directly in the PROT entry; the
//!   ten streaming dungeon scenes (an uncompressed MAN leading a typed-chunk
//!   stream - [`super::stream_man`]) also get the generalized rewriter: a
//!   longer line grows the MAN chunk, shifts the chunks after it, and either
//!   fits the entry's own sector slack or (with relayout) grows the entry.
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

use legaia_scene_formats::man_edit::{self, TextSite};

use legaia_disc_patch::disc::DiscPatcher;

use super::markup::{self, Target};
use super::monster_names;
use super::name_pool::NamePool;
use super::pack::{Entry, LanguagePack};
use super::segments;
use super::stream_man::StreamManText;
use super::ui;

mod exe_strings;
mod man_fit;
mod report;
mod scene_plan;

use exe_strings::{ScusStrPlan, plan_ui, record_code_moves};
pub(crate) use exe_strings::{apply_scus_work, overlay_strings, system_strings};
use man_fit::build_grown_stream_payload;
use report::SECTOR;
pub use report::{
    CarrierTrace, ImportPhase, ImportReport, ImportTrace, IssueKind, SceneTrace, SectionCounts,
    WritePath,
};
pub(crate) use scene_plan::plan_scene_man;
use scene_plan::{OPERAND_RUN_MSG, collect_ready};

/// Parsed provenance key.
pub(crate) enum Key {
    ScusStr {
        va: u32,
    },
    ScusParty {
        slot: usize,
    },
    /// A fixed `0x20`-byte NUL-padded SCUS cell (world-map place names).
    ScusCell {
        va: u32,
    },
    Man {
        entry: usize,
        off: usize,
    },
    Raw {
        entry: usize,
        off: usize,
    },
    Ui {
        prot: usize,
        va: u32,
    },
    /// A monster name inside its record in the monster archive.
    Mon {
        id: u16,
    },
}

pub(crate) fn parse_key(key: &str) -> Option<Key> {
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
            "cell" => {
                let va = it.next()?.strip_prefix("0x")?;
                Some(Key::ScusCell {
                    va: u32::from_str_radix(va, 16).ok()?,
                })
            }
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
        "mon" => Some(Key::Mon {
            id: monster_names::key_id(key)?,
        }),
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
        Ok(b) => {
            report.trace.encoded.insert(entry.key.clone(), b.len());
            Some(b)
        }
        Err(issues) => {
            let detail: Vec<String> = issues.iter().map(ToString::to_string).collect();
            report.issue_as(
                &entry.key,
                IssueKind::NotEncodable,
                format!("translation not encodable: {}", detail.join("; ")),
            );
            None
        }
    }
}

/// Budget check against the byte span actually measured on the disc.
fn fits(entry: &Entry, translated: &[u8], budget: usize, report: &mut ImportReport) -> bool {
    if translated.len() > budget {
        report.issue_as(
            &entry.key,
            IssueKind::OverBudget,
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
        report.issue_as(
            &entry.key,
            IssueKind::Mismatch,
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

/// Apply `pack` to the patcher's image. Untranslated entries are untouched.
/// No disc relayout: MANs whose full-length dialog overflows their compressed
/// footprint fall back to same-size + abbreviation.
pub fn import_pack(patcher: &mut DiscPatcher, pack: &LanguagePack) -> Result<ImportReport> {
    import_pack_phase(patcher, pack, ImportPhase::All, false)
}

/// [`import_pack`] with the whole-sector **disc relayout** enabled: a scene MAN
/// whose full-length dialog can't fit its compressed footprint is given `+N`
/// sectors (the PROT entry grows, the disc is relaid out) so the dialog imports
/// byte-faithfully instead of being abbreviated. See [`ImportReport`]'s
/// `relayout_*` counters.
pub fn import_pack_relayout(
    patcher: &mut DiscPatcher,
    pack: &LanguagePack,
) -> Result<ImportReport> {
    import_pack_phase(patcher, pack, ImportPhase::All, true)
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
    allow_relayout: bool,
) -> Result<ImportReport> {
    let mut report = ImportReport::default();
    // Typed accents become cells or folds per the pack's accent mode before
    // anything is encoded, so every path below sees the same text.
    let (prepared, accent_stats) = super::accents::prepared(pack);
    let pack: &LanguagePack = &prepared;
    report.accents = accent_stats;
    // Scene MAN entries whose full-length dialog overflows their compressed
    // footprint and needs a whole-sector disc relayout. Collected across the MAN
    // loop, then applied in one relayout pass so the PROT index space (and every
    // later index-keyed edit) is preserved.
    let mut pending_growth: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    let mut pending_meta: Vec<(usize, Vec<String>, u32)> = Vec::new();

    // Group the work by write mechanism.
    let mut scus_work: Vec<&Entry> = Vec::new();
    let mut man_work: BTreeMap<usize, Vec<(usize, &Entry)>> = BTreeMap::new();
    let mut raw_work: BTreeMap<usize, Vec<(usize, &Entry)>> = BTreeMap::new();
    let mut ui_work: BTreeMap<usize, Vec<(u32, &Entry)>> = BTreeMap::new();
    let mut mon_work: Vec<(u16, &Entry)> = Vec::new();
    for (_, entries) in pack.sections.iter() {
        for e in entries {
            let key = parse_key(&e.key);
            let in_phase = match (&key, phase) {
                (_, ImportPhase::All) => true,
                (Some(Key::Man { .. }) | Some(Key::Raw { .. }), ImportPhase::DialogOnly) => true,
                (
                    Some(Key::ScusStr { .. })
                    | Some(Key::ScusParty { .. })
                    | Some(Key::ScusCell { .. })
                    | Some(Key::Ui { .. })
                    | Some(Key::Mon { .. }),
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
                Some(Key::ScusStr { .. })
                | Some(Key::ScusParty { .. })
                | Some(Key::ScusCell { .. }) => scus_work.push(e),
                Some(Key::Man { entry, off }) => {
                    man_work.entry(entry).or_default().push((off, e));
                }
                Some(Key::Raw { entry, off }) => {
                    raw_work.entry(entry).or_default().push((off, e));
                }
                Some(Key::Ui { prot, va }) => {
                    ui_work.entry(prot).or_default().push((va, e));
                }
                Some(Key::Mon { id }) => mon_work.push((id, e)),
                None => report.issue(&e.key, "unrecognized key shape - skipped"),
            }
        }
    }

    // A Japanese disc frames its lines as count-led Shift-JIS (see
    // [`super::sjis`]); the write paths below speak the Latin `0x1F .. 0x00`
    // framing and USA executable addresses only. Its export is a reading
    // reference, so every filled entry is refused here and nothing is written.
    if super::build::detect(patcher).codec == super::build::TextCodec::ShiftJis {
        let refused = scus_work
            .iter()
            .copied()
            .chain(man_work.values().flatten().map(|(_, e)| *e))
            .chain(raw_work.values().flatten().map(|(_, e)| *e))
            .chain(ui_work.values().flatten().map(|(_, e)| *e))
            .chain(mon_work.iter().map(|(_, e)| *e));
        for e in refused {
            report.issue_as(
                &e.key,
                IssueKind::Refused,
                "Japanese disc: import writes to the Latin builds only \
                 (the Japanese export is a reading reference) - skipped",
            );
        }
        return Ok(report);
    }

    // SCUS strings. Read the file once; every write lands in the local copy
    // (so later verifications stay coherent with earlier writes) and is then
    // mirrored onto the disc.
    if !scus_work.is_empty() {
        let mut scus = patcher
            .read_named_file("SCUS_942.54")
            .context("SCUS_942.54 not found in disc image")?;
        // Measured before any write, so every span is the retail one.
        let pool = NamePool::build(&scus);
        let sys = system_strings(patcher, &scus);
        for (off, len) in apply_scus_work(&mut scus, &pool, Some(&sys), &scus_work, &mut report) {
            patcher.patch_named_file("SCUS_942.54", off as u64, &scus[off..off + len])?;
        }
    }

    // Scene-bundle MANs: one decompress -> N segment edits -> one repack per
    // PROT entry.
    for (entry_idx, edits) in man_work {
        let plan = plan_scene_man(patcher, entry_idx, &edits, allow_relayout, &mut report);
        for (off, bytes) in &plan.writes {
            patcher.patch_prot_entry(entry_idx, *off, bytes)?;
        }
        report.applied += plan.applied.len();
        report.applied_keys.extend(plan.applied);
        if let Some((payload, keys, grown_sectors)) = plan.growth {
            pending_growth.insert(entry_idx, payload);
            pending_meta.push((entry_idx, keys, grown_sectors));
        }
    }

    // Raw carriers: one read per PROT entry. A streaming dungeon scene (an
    // uncompressed MAN leading a typed-chunk stream) gets the generalized
    // rewriter - grow the MAN chunk, shift the later chunks, fit the entry's
    // own sector slack or (with relayout) grow the entry by whole sectors;
    // anything else, and any line the rewriter can't carry, is a same-size
    // in-place write.
    for (entry_idx, edits) in raw_work {
        let mut ct = CarrierTrace {
            footprint: patcher
                .entry_true_footprint_sectors(entry_idx)
                .map_or(0, |n| n as usize * SECTOR),
            keyed: edits.len(),
            ..Default::default()
        };
        'carrier: {
            let mut window = match patcher.read_entry(entry_idx) {
                Ok(b) => b,
                Err(e) => {
                    for (_, en) in &edits {
                        report.issue(&en.key, format!("PROT entry unreadable: {e}"));
                    }
                    break 'carrier;
                }
            };
            // Carrier gate: the `0x1F <text> 0x00` framing occurs by coincidence
            // throughout binary asset banks, so refuse to write into any entry
            // that isn't a genuine, prose-dense dialog carrier on the disc being
            // patched - writing a "translation" over a coincidental hit corrupts
            // the asset and freezes the game. Real event-script / dungeon-MAN
            // scenes clear the bar with a wide margin (see
            // [`segments::is_dialog_carrier`]).
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
                break 'carrier;
            }
            let sm = StreamManText::locate(&window);
            if let Some(sm) = &sm {
                ct.streaming = true;
                ct.sector_slack = patcher
                    .read_entry_footprint(entry_idx)
                    .ok()
                    .map(|foot| sm.sector_slack(&foot));
            }
            // Validate each segment against the disc once (framing /
            // already-applied / wrong-disc guard), collecting the ready set
            // with its current span.
            let ready = collect_ready(&window, entry_idx, &edits, &mut report);
            if ready.is_empty() {
                break 'carrier;
            }

            // Streaming dungeon MAN: the same structural gate as the LZS MANs
            // (keyed offsets are entry offsets; the MAN sits at `man_range()`),
            // and any line the walk does not reach stays same-size only.
            let (ready, blind) = match &sm {
                Some(sm) => {
                    let start = sm.man_range().start;
                    let mut walked = Vec::new();
                    let mut blind = Vec::new();
                    for r in ready {
                        match r
                            .off
                            .checked_sub(start)
                            .map(|o| man_edit::text_site(&sm.man, o))
                        {
                            Some(TextSite::Segment) => walked.push(r),
                            Some(TextSite::Operand) => report.issue(&r.entry.key, OPERAND_RUN_MSG),
                            _ => blind.push(r),
                        }
                    }
                    (walked, blind)
                }
                None => (Vec::new(), ready),
            };

            // Escape hatch: a line longer than its span grows the streaming
            // MAN. In the entry's own slack it is a same-size-image write
            // (PPF-safe); past it, a whole-sector grow staged for the relayout
            // pass. The same-size (blind) lines are written first so the grown
            // payload, built from a fresh read of the entry, carries them too.
            for r in &blind {
                if r.translated.len() <= r.old_len {
                    let padded = pad_segment(&r.translated, r.old_len);
                    window[r.off..r.off + r.old_len].copy_from_slice(&padded);
                    patcher.patch_prot_entry(entry_idx, r.off as u64, &padded)?;
                    report.applied += 1;
                    report.applied_keys.push(r.entry.key.clone());
                    ct.path = WritePath::Padded;
                }
            }
            if ready.iter().any(|r| r.translated.len() > r.old_len)
                && let Some(sm) = StreamManText::locate(&window)
                && let Some((payload, grown_sectors)) =
                    build_grown_stream_payload(patcher, entry_idx, &sm, &ready)
            {
                ct.grown_sectors = Some(grown_sectors);
                if grown_sectors == 0 {
                    patcher.patch_prot_entry(entry_idx, 0, &payload)?;
                    report.applied += ready.len();
                    report
                        .applied_keys
                        .extend(ready.iter().map(|r| r.entry.key.clone()));
                    ct.path = WritePath::Relocated;
                    break 'carrier;
                }
                if allow_relayout {
                    pending_growth.insert(entry_idx, payload);
                    pending_meta.push((
                        entry_idx,
                        ready.iter().map(|r| r.entry.key.clone()).collect(),
                        grown_sectors,
                    ));
                    ct.path = WritePath::Relayout;
                    break 'carrier;
                }
            }

            // Same-size path: the fitting lines in place, space-padded; the
            // rest reported (the carrier couldn't be grown to fit them).
            for r in ready
                .iter()
                .chain(blind.iter().filter(|r| r.translated.len() > r.old_len))
            {
                if r.translated.len() > r.old_len {
                    report.issue_as(
                        &r.entry.key,
                        IssueKind::OverBudget,
                        format!(
                            "translation needs {} bytes but the in-place budget is {} and the \
                             scene MAN could not be grown to fit it (shorten this line)",
                            r.translated.len(),
                            r.old_len
                        ),
                    );
                    continue;
                }
                let padded = pad_segment(&r.translated, r.old_len);
                window[r.off..r.off + r.old_len].copy_from_slice(&padded);
                patcher.patch_prot_entry(entry_idx, r.off as u64, &padded)?;
                report.applied += 1;
                report.applied_keys.push(r.entry.key.clone());
                ct.path = WritePath::Padded;
            }
        }
        report.trace.carriers.insert(entry_idx, ct);
    }

    // Apply all staged whole-sector grows (scene-bundle MANs and streaming
    // carriers alike) in one disc relayout, so the PROT index space (and every
    // later index-keyed edit) is preserved. Same-size edits above are already
    // in the image and carried through the rebuild.
    if !pending_growth.is_empty() {
        match patcher.grow_prot_entries(&pending_growth) {
            Ok(()) => {
                for (_, keys, grown_sectors) in &pending_meta {
                    report.relayout_entries += 1;
                    report.relayout_sectors_added += grown_sectors;
                    report.applied += keys.len();
                    report.applied_keys.extend(keys.iter().cloned());
                }
            }
            Err(e) => {
                for (entry_idx, keys, _) in &pending_meta {
                    for k in keys {
                        report.issue(k, format!("scene {entry_idx}: disc relayout failed: {e}"));
                    }
                    if let Some(t) = report.trace.scenes.get_mut(entry_idx) {
                        t.path = WritePath::None;
                    }
                    if let Some(t) = report.trace.carriers.get_mut(entry_idx) {
                        t.path = WritePath::None;
                    }
                }
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
        // Measured before any write, so every span is the retail one.
        let cs = overlay_strings(patcher, prot, &buf, base_va);
        let mut grow: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut grow_entries: BTreeMap<u32, &Entry> = BTreeMap::new();
        for (va, en) in edits {
            let strict = ui::pool_for(prot, va).is_some_and(|p| p.strict);
            match plan_ui(
                prot,
                &buf,
                base_va,
                en,
                va,
                strict,
                cs.is_movable(va),
                &mut report,
            ) {
                Some(ScusStrPlan::Write(off, bytes)) => {
                    patcher.patch_prot_entry(prot, off as u64, &bytes)?;
                    buf[off..off + bytes.len()].copy_from_slice(&bytes);
                    report.applied += 1;
                    report.applied_keys.push(en.key.clone());
                }
                Some(ScusStrPlan::Grow(bytes)) => {
                    grow.push((va, bytes));
                    grow_entries.insert(va, en);
                }
                None => {}
            }
        }
        // Labels that outgrew their span: move them, with every reference,
        // into the room the pools' compaction frees or the ledger's
        // translation region in this image.
        if !grow.is_empty() {
            let extra = legaia_disc_patch::space_ledger::translation_spans(
                legaia_disc_patch::space_ledger::Image::Prot(prot),
                &buf,
                base_va,
            );
            let reloc = cs.relocate(&mut buf, &grow, &extra);
            for &(off, len) in &reloc.written {
                patcher.patch_prot_entry(prot, off as u64, &buf[off..off + len])?;
            }
            record_code_moves(&reloc, prot, &grow, &grow_entries, &mut report);
        }
    }

    if !mon_work.is_empty() {
        import_monster_names(patcher, &mon_work, &mut report)?;
    }

    // The accent font rides with the names phase (the executable's advance
    // table is written there), once per import.
    if super::accents::AccentMode::of(pack) == super::accents::AccentMode::Font
        && matches!(phase, ImportPhase::All | ImportPhase::NamesOnly)
    {
        report.accent_font = Some(super::accents::apply_accent_font(patcher)?);
    }

    Ok(report)
}

/// Monster names: one decode / rewrite / re-pack per touched record, into its
/// fixed slot of the archive (see [`monster_names`]). The budget is re-derived
/// from the archive on this disc, never taken from the pack.
fn import_monster_names(
    patcher: &mut DiscPatcher,
    work: &[(u16, &Entry)],
    report: &mut ImportReport,
) -> Result<()> {
    let mut archive = match patcher.read_entry(legaia_disc_patch::disc::MONSTER_ARCHIVE_ENTRY) {
        Ok(b) => b,
        Err(e) => {
            for (_, en) in work {
                report.issue(&en.key, format!("monster archive unreadable: {e}"));
            }
            return Ok(());
        }
    };
    let fields = monster_names::fields(&archive);
    let budgets: BTreeMap<u16, usize> = monster_names::budgets(&fields).into_iter().collect();
    let current: BTreeMap<u16, Vec<u8>> = fields.into_iter().map(|(id, f)| (id, f.bytes)).collect();
    for &(id, en) in work {
        let (Some(cur), Some(&budget)) = (current.get(&id), budgets.get(&id)) else {
            report.issue(&en.key, "no named monster record at this id - skipped");
            continue;
        };
        let Ok(source) = encode_source(en, Target::CString, report) else {
            continue;
        };
        let Some(translated) = encode_translation(en, Target::CString, report) else {
            continue;
        };
        if !translated.iter().all(|&b| (0x20..0x7F).contains(&b)) {
            report.issue_as(
                &en.key,
                IssueKind::NotEncodable,
                "a monster name takes printable glyphs only (the loader reads the \
                 record's name as plain text, markup escapes included)",
            );
            continue;
        }
        if cur == &translated {
            report.already_applied += 1;
            report.already_keys.push(en.key.clone());
            continue;
        }
        if let Some(src) = &source
            && src != cur
        {
            report.issue_as(
                &en.key,
                IssueKind::Mismatch,
                "disc bytes don't match the pack source (different disc revision or \
                 a conflicting patch) - skipped",
            );
            continue;
        }
        report.room(&en.key, budget);
        if source.is_none() && !hint_agrees(en, budget, report) {
            continue;
        }
        // Past the record's own room the record grows (up to the longest
        // retail name); `rewrite_slot` enforces both limits.
        match monster_names::rewrite_slot(&archive, id, &translated) {
            Ok((slot, grown)) => {
                let at = monster_names::slot_offset(id);
                patcher.patch_monster_slot(id, &slot)?;
                archive[at..at + slot.len()].copy_from_slice(&slot);
                report.applied += 1;
                report.grown_monster_names += usize::from(grown);
                report.applied_keys.push(en.key.clone());
                if grown {
                    report.trace.grown_monsters.insert(en.key.clone());
                }
            }
            Err(e) => report.issue_as(&en.key, IssueKind::Refused, format!("{e} - skipped")),
        }
    }
    Ok(())
}
