//! Per-scene dialog planning: encoding each translated segment, collecting the
//! MANs that are ready to write, gating the text sites, and turning a scene's
//! pack entries into MAN edits.

use super::super::export::SceneManText;
use super::super::markup::Target;
use super::super::pack::Entry;
use super::super::segments;
use super::man_fit::{
    GrowFail, build_grown_entry_payload, fit_man_in_footprint, size_word_write, stage_fitted_man,
    try_grow_man,
};
use super::report::{ImportReport, IssueKind, SceneTrace, WritePath, sectors_for_overflow};
use super::{encode_source, encode_translation, hint_agrees, pad_segment};
use legaia_disc_patch::disc::DiscPatcher;
use legaia_scene_formats::man_edit::{self, TextSite};

/// A dialog segment inside a scene MAN, validated against the disc and ready
/// to write: its decompressed-domain offset, current on-disc byte length (the
/// `0x1F .. 0x00` framing span), and the encoded translated bytes.
pub(super) struct ReadyMan<'a> {
    pub(super) off: usize,
    pub(super) old_len: usize,
    pub(super) translated: Vec<u8>,
    pub(super) entry: &'a Entry,
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

/// Validate a dialog segment (in a decompressed MAN or a raw PROT entry)
/// against the bytes actually on the disc - framing, already-applied,
/// wrong-disc guard - *without* mutating, returning the current framing span
/// so the caller can choose the same-size or grow path. The segment's byte
/// budget is its own `0x1F <text> 0x00` framing on this disc, never a number
/// the pack asserts, so a bad pack can't overrun the text pool it edits.
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
    report.room(&entry.key, old_len);
    let cur = &buf[off..term];
    // Already applied: exact (a grown/shrunk write) or space-padded (same-size).
    if cur == translated || cur == pad_segment(translated, old_len).as_slice() {
        report.already_applied += 1;
        report.already_keys.push(entry.key.clone());
        return SegPrep::Already;
    }
    match source {
        Some(src) if cur != src => {
            report.issue_as(
                &entry.key,
                IssueKind::Mismatch,
                "disc bytes don't match the pack source (different disc revision or \
                 a conflicting patch) - skipped",
            );
            SegPrep::Skip
        }
        None if !hint_agrees(entry, old_len, report) => SegPrep::Skip,
        _ => SegPrep::Ready { old_len },
    }
}

/// Validate every keyed line of one carrier against the disc
/// ([`prepare_segment`]) and return the ready set.
///
/// A **translation-only** pack has no source text to prove a line is the one
/// it was written for - only the segment's framing and its length hint - so
/// once any keyed line of a carrier fails that check, the carrier's text is
/// not where the pack expects it (an earlier import relocated the scene, or
/// another patch moved it), and a line that still happens to pass may be a
/// *neighbour* of the right length. The whole carrier is then skipped: that
/// is what makes re-importing onto an already translated image a no-op
/// rather than a write into shifted text. A working pack keeps the per-line
/// source comparison, which cannot land on a neighbour.
pub(super) fn collect_ready<'a>(
    buf: &[u8],
    entry_idx: usize,
    edits: &[(usize, &'a Entry)],
    report: &mut ImportReport,
) -> Vec<ReadyMan<'a>> {
    let mut ready = Vec::new();
    let mut mismatched = 0usize;
    for (off, en) in edits {
        let Ok(source) = encode_source(en, Target::Segment, report) else {
            continue;
        };
        let Some(translated) = encode_translation(en, Target::Segment, report) else {
            continue;
        };
        match prepare_segment(buf, en, *off, source.as_deref(), &translated, report) {
            SegPrep::Ready { old_len } => ready.push(ReadyMan {
                off: *off,
                old_len,
                translated,
                entry: en,
            }),
            SegPrep::Skip if source.is_none() => mismatched += 1,
            _ => {}
        }
    }
    if mismatched > 0 && !ready.is_empty() {
        for r in &ready {
            report.issue_as(
                &r.entry.key,
                IssueKind::Mismatch,
                format!(
                    "PROT entry {entry_idx}: {mismatched} other line(s) of this scene no longer \
                     sit where the pack expects them (an earlier import or another patch moved \
                     its text) - the whole scene is skipped so no line lands on a neighbour"
                ),
            );
        }
        return Vec::new();
    }
    ready
}

/// Diagnostic for a keyed line whose `0x1F` framing is a coincidental byte
/// run inside a decoded instruction's operands.
pub(super) const OPERAND_RUN_MSG: &str = "the text framing at this offset is a coincidence inside a \
     decoded instruction's operands (an actor index followed by printable bytes), not a \
     dialog segment - skipped (a write here would corrupt the script)";

/// Same-size lines pre-applied into a decompressed MAN: `(offset, previous
/// bytes, entry)`, the rollback shape the same-size path uses.
pub(super) type PreApplied<'a> = Vec<(usize, Vec<u8>, &'a Entry)>;

/// Partition the ready lines of one decompressed MAN by [`TextSite`]: lines on
/// a clean-walk text segment come back as the relocatable `ready` set; a line
/// whose framing is an instruction's operand bytes is refused with a
/// diagnostic; a line the walk does not reach is same-size only - applied into
/// `decoded` here (space-padded) when it fits, and reported when it does not.
/// The pre-applied `(offset, previous bytes, entry)` triples are returned so
/// the caller counts them once a write happens and can roll them back.
fn gate_text_sites<'a>(
    decoded: &mut [u8],
    ready: Vec<ReadyMan<'a>>,
    report: &mut ImportReport,
) -> (Vec<ReadyMan<'a>>, PreApplied<'a>) {
    let mut walked = Vec::with_capacity(ready.len());
    let mut applied = Vec::new();
    for r in ready {
        match man_edit::text_site(decoded, r.off) {
            TextSite::Segment => walked.push(r),
            TextSite::Operand => report.issue(&r.entry.key, OPERAND_RUN_MSG),
            TextSite::Unreached | TextSite::NoRecord => {
                if r.translated.len() > r.old_len {
                    report.issue_as(
                        &r.entry.key,
                        IssueKind::OverBudget,
                        format!(
                            "translation needs {} bytes but the in-place budget is {} and \
                             the script walk does not reach this line, so it cannot be \
                             relocated (shorten this line)",
                            r.translated.len(),
                            r.old_len
                        ),
                    );
                    continue;
                }
                let before = decoded[r.off..r.off + r.old_len].to_vec();
                decoded[r.off..r.off + r.old_len]
                    .copy_from_slice(&pad_segment(&r.translated, r.old_len));
                applied.push((r.off, before, r.entry));
            }
        }
    }
    (walked, applied)
}

/// What one scene MAN's import decided ([`plan_scene_man`]): the writes to
/// land in its PROT entry, the keys they carry, and a staged whole-sector
/// grow.
#[derive(Default)]
pub(crate) struct ScenePlan {
    /// `(entry offset, bytes)` writes inside the entry's footprint.
    pub writes: Vec<(u64, Vec<u8>)>,
    /// Keys the writes carry (counted applied once they land).
    pub applied: Vec<String>,
    /// A staged relayout: `(full-footprint payload, keys, grown sectors)`.
    pub growth: Option<(Vec<u8>, Vec<String>, u32)>,
}

/// Plan one scene MAN's dialog edits (`edits` = `(decompressed offset,
/// entry)` pairs of its filled `man:` keys) without touching the disc: the
/// relocator at full length, a staged relayout, a fitted rollback, or the
/// padded same-size write, in that order. Records the scene's
/// [`SceneTrace`] and every per-key diagnostic in `report`. Import applies
/// the plan; [`super::super::space::scene_fit`] only reads it.
pub(crate) fn plan_scene_man(
    patcher: &DiscPatcher,
    entry_idx: usize,
    edits: &[(usize, &Entry)],
    allow_relayout: bool,
    report: &mut ImportReport,
) -> ScenePlan {
    let mut plan = ScenePlan::default();
    let entry_bytes = match patcher.read_entry(entry_idx) {
        Ok(b) => b,
        Err(e) => {
            for (_, en) in edits {
                report.issue(&en.key, format!("PROT entry unreadable: {e}"));
            }
            return plan;
        }
    };
    let Some(mut man) = SceneManText::locate(&entry_bytes) else {
        for (_, en) in edits {
            report.issue(&en.key, "scene MAN not found in this PROT entry - skipped");
        }
        return plan;
    };
    let mut trace = SceneTrace {
        footprint: man.compressed_budget,
        disc_len: man.compressed_len,
        keyed: edits.len(),
        ..Default::default()
    };
    plan_man_edits(
        patcher,
        entry_idx,
        &mut man,
        edits,
        allow_relayout,
        report,
        &mut plan,
        &mut trace,
    );
    report.trace.scenes.insert(entry_idx, trace);
    plan
}

/// The body of [`plan_scene_man`] once the MAN is located.
#[allow(clippy::too_many_arguments)]
fn plan_man_edits(
    patcher: &DiscPatcher,
    entry_idx: usize,
    man: &mut SceneManText,
    edits: &[(usize, &Entry)],
    allow_relayout: bool,
    report: &mut ImportReport,
    plan: &mut ScenePlan,
    trace: &mut SceneTrace,
) {
    // Validate each segment against the disc once (framing / already-applied
    // / wrong-disc guard), collecting the ready set with its current span.
    let ready = collect_ready(&man.decoded, entry_idx, edits, report);
    // Structural gate: a line is dialog only when its `0x1F` lead is the
    // text an instruction on the record's clean script walk carries. A
    // coincidental `1F .. 00` inside an instruction's operands is refused on
    // every path (writing "text" there corrupts the script); a line the walk
    // does not reach is written same-size only, pre-applied here so the grown
    // MAN carries it and the same-size rollback covers it.
    let (ready, mut applied) = gate_text_sites(&mut man.decoded, ready, report);
    if ready.is_empty() && applied.is_empty() {
        return;
    }
    let any_over = ready.iter().any(|r| r.translated.len() > r.old_len);

    // Escape hatch: if any line overflows its own byte span, try to grow the
    // whole MAN (budget = the MAN's own on-disc footprint, not each string).
    // On success the segment framing moves but every crossing reference is
    // relocated, so the pager still walks it correctly.
    if any_over {
        match try_grow_man(man, &ready) {
            Ok((stream, new_size)) => {
                trace.path = WritePath::Relocated;
                trace.written_len = Some(stream.len());
                plan.writes.push((man.man_offset as u64, stream));
                plan.writes.push(size_word_write(man, new_size));
                plan.applied
                    .extend(ready.iter().map(|r| r.entry.key.clone()));
                plan.applied
                    .extend(applied.iter().map(|(_, _, en)| en.key.clone()));
                return;
            }
            Err(GrowFail::Overflow(n)) => trace.full_overflow = Some(n),
            Err(GrowFail::NotPreserved) => trace.refused = true,
        }
        // The full-length dialog overflows the MAN's compressed footprint.
        // With relayout enabled, stage a whole-sector grow of this PROT entry
        // (applied together, after the loop) instead of abbreviating.
        if allow_relayout
            && let Some((payload, grown_sectors, stream_len)) =
                build_grown_entry_payload(patcher, entry_idx, man, &ready)
        {
            trace.path = WritePath::Relayout;
            trace.relayout_sectors = Some(grown_sectors);
            trace.written_len = Some(stream_len);
            plan.growth = Some((
                payload,
                ready
                    .iter()
                    .map(|r| r.entry.key.clone())
                    .chain(applied.iter().map(|(_, _, en)| en.key.clone()))
                    .collect(),
                grown_sectors,
            ));
            return;
        }
        if !allow_relayout && let Some(n) = trace.full_overflow {
            trace.relayout_would_add = Some(sectors_for_overflow(n));
        }

        // A line is over its span and neither the whole-set grow nor a
        // relayout took the scene: fit the most lines the footprint holds
        // through the relocator before falling back to padding (which could
        // only drop every over-span line).
        let fit = fit_man_in_footprint(man, &ready);
        trace.refused |= fit.refused;
        if let Some(fit) = fit.fitted {
            stage_fitted_man(entry_idx, man, &ready, &applied, fit, report, plan, trace);
            return;
        }
    }

    // Same-size (fast, byte-identical) path: apply the fitting lines in
    // place, report the over-budget ones (the MAN couldn't be grown to fit
    // them), then recompress with a longest-first rollback if the scene's
    // dialog no longer fits its compressed footprint.
    let pre_applied = applied.len();
    for r in &ready {
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
        let before = man.decoded[r.off..r.off + r.old_len].to_vec();
        let padded = pad_segment(&r.translated, r.old_len);
        man.decoded[r.off..r.off + r.old_len].copy_from_slice(&padded);
        applied.push((r.off, before, r.entry));
    }
    if applied.is_empty() {
        return;
    }

    // The MAN must recompress into its original footprint. Translated text
    // is less repetitive than the source, so a scene can overflow by a few
    // bytes; roll back the costliest lines (longest first) one at a time
    // rather than losing the whole scene's dialog. `pop()` takes from the
    // vector's tail, so an ASCENDING sort puts the longest line there.
    let mut stream = man.repack_measured();
    if let Err(over) = stream {
        trace.padded_overflow = Some(over);
    }
    // Space padding spends bytes the footprint may not have: every shorter
    // translation padded back to the source length recompresses worse than
    // the same line at its own length. Before rolling lines back, undo the
    // padding and let the relocator fit the scene at exact lengths (the
    // shape a translation-only pack of in-budget lines hits).
    if stream.is_err() && !any_over && applied.len() > pre_applied {
        for (off, before, _) in applied.drain(pre_applied..).rev() {
            man.decoded[off..off + before.len()].copy_from_slice(&before);
        }
        let fit = fit_man_in_footprint(man, &ready);
        trace.refused |= fit.refused;
        if trace.full_overflow.is_none() {
            trace.full_overflow = fit.first_overflow;
        }
        if let Some(fit) = fit.fitted {
            stage_fitted_man(entry_idx, man, &ready, &applied, fit, report, plan, trace);
            return;
        }
        for r in &ready {
            let before = man.decoded[r.off..r.off + r.old_len].to_vec();
            man.decoded[r.off..r.off + r.old_len]
                .copy_from_slice(&pad_segment(&r.translated, r.old_len));
            applied.push((r.off, before, r.entry));
        }
    }
    if stream.is_err() {
        applied.sort_by_key(|(_, before, _)| before.len());
        while stream.is_err()
            && let Some((off, before, en)) = applied.pop()
        {
            man.decoded[off..off + before.len()].copy_from_slice(&before);
            report.issue_as(
                &en.key,
                IssueKind::RolledBack,
                format!(
                    "scene {entry_idx}: rolled back - the scene's dialog no longer \
                     recompresses into its {} byte footprint (shorten this line)",
                    man.compressed_budget
                ),
            );
            trace.rolled_back.push(en.key.clone());
            if applied.is_empty() {
                break;
            }
            stream = man.repack_measured();
        }
    }
    if let Ok(stream) = stream
        && !applied.is_empty()
    {
        trace.path = WritePath::Padded;
        trace.written_len = Some(stream.len());
        plan.writes.push((man.man_offset as u64, stream));
        plan.applied
            .extend(applied.iter().map(|(_, _, en)| en.key.clone()));
    }
}
