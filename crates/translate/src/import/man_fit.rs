//! Getting an edited MAN back onto the disc: relocate-and-pack, fitting inside
//! the entry's footprint, staging the fitted bytes, and building the grown
//! entry or stream payload when it cannot fit.

use super::super::export::SceneManText;
use super::super::stream_man::StreamManText;
use super::report::{ImportReport, IssueKind, SECTOR, SceneTrace, WritePath, sectors_for_overflow};
use super::scene_plan::{PreApplied, ReadyMan, ScenePlan};
use anyhow::Result;
use legaia_disc_patch::disc::DiscPatcher;
use legaia_scene_formats::man_edit::{self, TextEdit};
use legaia_scene_formats::scene_asset_table;

/// Why [`relocate_and_pack`] could not produce a stream.
pub(super) enum GrowFail {
    /// The relocator refused the edit set, or the rewrite is not the same
    /// program relocated - nothing about *which* lines are kept changes that.
    NotPreserved,
    /// The relocated MAN recompresses `n` bytes past the footprint.
    Overflow(usize),
}

/// Attempt the **generalized rewriter** path for one scene MAN: grow/shrink
/// every given segment to its exact translated bytes, relocate all crossing
/// references ([`man_edit::apply_text_edits`]), verify the rewrite is the same
/// program relocated ([`man_edit::text_edits_preserve_scripts`]), and recompress
/// within the MAN's on-disc footprint (fast greedy parse, then the optimal
/// parse when it just misses - same policy as `repack`). Returns
/// `(recompressed_stream, new_decompressed_size)`.
fn relocate_and_pack(
    man: &SceneManText,
    ready: &[&ReadyMan],
) -> std::result::Result<(Vec<u8>, u32), GrowFail> {
    let edits: Vec<TextEdit> = ready
        .iter()
        .map(|r| TextEdit {
            offset: r.off,
            old_len: r.old_len,
            new_bytes: r.translated.clone(),
        })
        .collect();
    let grown =
        man_edit::apply_text_edits(&man.decoded, &edits).map_err(|_| GrowFail::NotPreserved)?;
    if !man_edit::text_edits_preserve_scripts(&man.decoded, &grown) {
        return Err(GrowFail::NotPreserved);
    }
    let stream = legaia_lzs::compress(&grown);
    if stream.len() <= man.compressed_budget {
        return Ok((stream, grown.len() as u32));
    }
    let opt = legaia_lzs::compress_optimal(&grown);
    if opt.len() <= man.compressed_budget {
        return Ok((opt, grown.len() as u32));
    }
    Err(GrowFail::Overflow(opt.len() - man.compressed_budget))
}

/// [`relocate_and_pack`] over every ready line; the error says why the
/// growth can't be done safely / won't fit (caller falls back to relayout or
/// same-size).
pub(super) fn try_grow_man(
    man: &SceneManText,
    ready: &[ReadyMan],
) -> Result<(Vec<u8>, u32), GrowFail> {
    let all: Vec<&ReadyMan> = ready.iter().collect();
    relocate_and_pack(man, &all)
}

/// A same-size scene write the relocator fitted into the MAN's footprint:
/// the stream, its decompressed size, and which `ready` lines it carries
/// (`kept`) versus left English (`dropped`).
pub(super) struct FittedMan {
    stream: Vec<u8>,
    size: u32,
    kept: Vec<usize>,
    dropped: Vec<usize>,
}

/// What [`fit_man_in_footprint`] found.
pub(super) struct FitOutcome {
    /// The fitted write, or `None` when the relocator refused the scene or
    /// no subset fits.
    pub(super) fitted: Option<FittedMan>,
    /// Compressed bytes over the footprint of the first attempt (every line
    /// at its exact length), when it overflowed.
    pub(super) first_overflow: Option<usize>,
    /// The relocator refused the edit set.
    pub(super) refused: bool,
}

/// Fit as many `ready` lines as possible into the MAN's **own** compressed
/// footprint through the relocator (every line at its exact length - a
/// shorter translation shrinks the MAN, a longer one grows it), rolling
/// lines back to the source text only when the scene still overflows.
///
/// This is the same-size image's best path whenever the space-padded write
/// does not recompress: padding every shorter line back to the English
/// length spends bytes the footprint does not have, while the exact-length
/// rewrite gives them back. The rollback drops the lines that grow the MAN
/// most first, a batch at a time sized by the measured overflow, so a scene
/// that is a few bytes over loses a line or two rather than every line
/// longer than the one that tipped it. No fit when the relocator refuses the
/// scene outright or no subset fits (the caller keeps the padded path).
pub(super) fn fit_man_in_footprint(man: &SceneManText, ready: &[ReadyMan]) -> FitOutcome {
    use std::cmp::Reverse;
    let growth = |r: &ReadyMan| r.translated.len() as isize - r.old_len as isize;
    // Drop order: biggest growth first, then the longest translation.
    let mut order: Vec<usize> = (0..ready.len()).collect();
    order.sort_by_key(|&i| {
        (
            Reverse(growth(&ready[i])),
            Reverse(ready[i].translated.len()),
            i,
        )
    });
    let mut out = FitOutcome {
        fitted: None,
        first_overflow: None,
        refused: false,
    };
    let mut keep = vec![true; ready.len()];
    let mut cursor = 0usize;
    loop {
        let kept: Vec<usize> = (0..ready.len()).filter(|&i| keep[i]).collect();
        if kept.is_empty() {
            return out;
        }
        let refs: Vec<&ReadyMan> = kept.iter().map(|&i| &ready[i]).collect();
        match relocate_and_pack(man, &refs) {
            Ok((stream, size)) => {
                let dropped = (0..ready.len()).filter(|&i| !keep[i]).collect();
                out.fitted = Some(FittedMan {
                    stream,
                    size,
                    kept,
                    dropped,
                });
                return out;
            }
            Err(GrowFail::NotPreserved) => {
                out.refused = true;
                return out;
            }
            Err(GrowFail::Overflow(over)) => {
                out.first_overflow.get_or_insert(over);
                // Raw bytes restored are an upper bound on the compressed
                // bytes saved, so a batch whose growth covers the overflow is
                // the fewest lines that can possibly fit - never more.
                let mut freed = 0usize;
                while freed < over && cursor < order.len() {
                    let i = order[cursor];
                    cursor += 1;
                    keep[i] = false;
                    freed += growth(&ready[i]).max(1) as usize;
                }
            }
        }
    }
}

/// Stage a [`fit_man_in_footprint`] result: the stream, the MAN descriptor's
/// new size word, the carried lines (plus the same-size `pre_applied` ones
/// already in the decoded MAN) as applied, and a rollback diagnostic for
/// every line left in the source language.
#[allow(clippy::too_many_arguments)]
pub(super) fn stage_fitted_man(
    entry_idx: usize,
    man: &SceneManText,
    ready: &[ReadyMan],
    pre_applied: &PreApplied,
    fit: FittedMan,
    report: &mut ImportReport,
    plan: &mut ScenePlan,
    trace: &mut SceneTrace,
) {
    trace.path = WritePath::Relocated;
    trace.written_len = Some(fit.stream.len());
    plan.writes.push((man.man_offset as u64, fit.stream));
    plan.writes.push(size_word_write(man, fit.size));
    plan.applied
        .extend(fit.kept.iter().map(|&i| ready[i].entry.key.clone()));
    plan.applied
        .extend(pre_applied.iter().map(|(_, _, en)| en.key.clone()));
    for &i in &fit.dropped {
        let key = &ready[i].entry.key;
        report.issue_as(
            key,
            IssueKind::RolledBack,
            format!(
                "scene {entry_idx}: rolled back - the scene's dialog no longer \
                 recompresses into its {} byte footprint (shorten this line)",
                man.compressed_budget
            ),
        );
        trace.rolled_back.push(key.clone());
    }
}

/// The MAN descriptor's `(type << 24) | size` word rewritten for a MAN of
/// `size` decompressed bytes, as an entry write.
pub(super) fn size_word_write(man: &SceneManText, size: u32) -> (u64, Vec<u8>) {
    (
        man.man_descriptor_off as u64,
        scene_asset_table::encode_size_word(0x03, size)
            .to_le_bytes()
            .to_vec(),
    )
}

/// Build entry `entry_idx`'s new **full-footprint payload**, growing the scene
/// MAN by whole sectors so its full-length dialog fits. Used only when the
/// in-place grow ([`try_grow_man`]) overflowed the MAN's compressed footprint and
/// a disc relayout is permitted.
///
/// Steps (see `docs/tooling/pal-localizations.md`): relocate all crossing
/// references + verify the program is preserved, recompress with no budget cap,
/// then within the entry insert `ceil(overflow / 2048)` sectors after the MAN's
/// compressed region, shift every later sub-asset, and bump their
/// `scene_asset_table` descriptor `data_offset`s (+ the MAN decompressed-size
/// word). Returns `(new_footprint_payload, grown_sectors, stream_len)`, or
/// `None` when the grow can't be done safely.
pub(super) fn build_grown_entry_payload(
    patcher: &DiscPatcher,
    entry_idx: usize,
    man: &SceneManText,
    ready: &[ReadyMan],
) -> Option<(Vec<u8>, u32, usize)> {
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
    let greedy = legaia_lzs::compress(&grown);
    let optimal = legaia_lzs::compress_optimal(&grown);
    let stream = if optimal.len() < greedy.len() {
        optimal
    } else {
        greedy
    };

    let extra = stream.len().checked_sub(man.compressed_budget)?;
    if extra == 0 {
        return None; // fits in place; caller should have used try_grow_man
    }
    let grown_sectors = sectors_for_overflow(extra) as usize;
    let insert = grown_sectors * SECTOR;

    // The entry's TRUE footprint bytes (the real per-scene allocation), not the
    // indexed-size over-read `read_entry` returns.
    let foot = patcher.read_entry_footprint(entry_idx).ok()?;
    let table = scene_asset_table::detect(&foot)?;
    let man_i = table.descriptor_index(0x03)?;
    let man_off = man.man_offset;
    let next_sub_off = man_off.checked_add(man.compressed_budget)?;
    if next_sub_off > foot.len() || man_off + stream.len() > next_sub_off + insert {
        return None;
    }

    // Insert `insert` blank bytes at the MAN's compressed-region end; later
    // sub-assets shift up by `insert`.
    let mut new = Vec::with_capacity(foot.len() + insert);
    new.extend_from_slice(&foot[..next_sub_off]);
    new.resize(new.len() + insert, 0);
    new.extend_from_slice(&foot[next_sub_off..]);
    new[man_off..man_off + stream.len()].copy_from_slice(&stream);

    // Bump the descriptor `data_offset` of every sub-asset after the MAN.
    for (i, d) in table.used().iter().enumerate() {
        if d.data_offset as usize > man_off {
            let off = scene_asset_table::SceneAssetTable::size_word_offset(i) + 4;
            if off + 4 > new.len() {
                return None;
            }
            let nv = d.data_offset + insert as u32;
            new[off..off + 4].copy_from_slice(&nv.to_le_bytes());
        }
    }
    // Rewrite the MAN descriptor's decompressed-size word.
    let sw = scene_asset_table::SceneAssetTable::size_word_offset(man_i);
    if sw + 4 > new.len() {
        return None;
    }
    let size_word = scene_asset_table::encode_size_word(0x03, grown.len() as u32);
    new[sw..sw + 4].copy_from_slice(&size_word.to_le_bytes());

    debug_assert_eq!(new.len(), foot.len() + insert);
    debug_assert_eq!(new.len() % SECTOR, 0);
    Some((new, grown_sectors as u32, stream.len()))
}

/// Build a streaming scene entry's new **full-footprint payload** with its
/// leading MAN chunk grown to carry every ready line at full length (see
/// [`StreamManText`]). `ready` offsets are entry offsets (the `raw:` key
/// space); each must lie inside the MAN chunk. Returns `(payload,
/// grown_sectors)` - `grown_sectors == 0` when the entry's own trailing sector
/// slack absorbs the growth, so the payload can be written in place - or
/// `None` when the rewrite can't be done safely (a line outside the MAN, a
/// record the relocator refuses, a program the round-trip doesn't preserve,
/// or the loader arena bound).
pub(super) fn build_grown_stream_payload(
    patcher: &DiscPatcher,
    entry_idx: usize,
    sm: &StreamManText,
    ready: &[ReadyMan],
) -> Option<(Vec<u8>, u32)> {
    let range = sm.man_range();
    let mut edits = Vec::with_capacity(ready.len());
    for r in ready {
        if r.off < range.start || r.off + r.old_len > range.end {
            return None;
        }
        edits.push(TextEdit {
            offset: r.off - range.start,
            old_len: r.old_len,
            new_bytes: r.translated.clone(),
        });
    }
    let grown = man_edit::apply_text_edits(&sm.man, &edits).ok()?;
    if !man_edit::text_edits_preserve_scripts(&sm.man, &grown) {
        return None;
    }
    let foot = patcher.read_entry_footprint(entry_idx).ok()?;
    // The rebuilt payload is never shorter than the footprint (a shrunken
    // MAN keeps its sectors), so the difference is the whole-sector growth.
    let payload = sm.rebuild(&foot, &grown)?;
    let grown_sectors = (payload.len() / SECTOR).checked_sub(foot.len() / SECTOR)?;
    Some((payload, grown_sectors as u32))
}
