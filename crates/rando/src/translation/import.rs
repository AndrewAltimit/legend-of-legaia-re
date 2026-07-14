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
//! Before writing, each target is verified against the pack's `source`: if
//! the disc bytes already equal the translation the entry counts as already
//! applied (idempotent re-import); if they match neither, the entry is
//! skipped with a warning (wrong disc / conflicting patch). Encode failures
//! (non-Latin characters, over-budget text) are reported per entry with
//! per-character positions and leave the disc untouched.

use std::collections::BTreeMap;

use anyhow::{Context, Result};

use legaia_asset::{item_names, new_game};

use crate::disc::DiscPatcher;

use super::export::SceneManText;
use super::markup::{self, Target};
use super::pack::{Entry, LanguagePack};

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
}

impl ImportReport {
    fn issue(&mut self, key: &str, msg: impl Into<String>) {
        self.issues.push((key.to_string(), msg.into()));
    }
}

/// Parsed provenance key.
enum Key {
    ScusStr { va: u32 },
    ScusParty { slot: usize },
    Man { entry: usize, off: usize },
    Raw { entry: usize, off: usize },
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
        _ => None,
    }
}

/// Encode `entry.translation` for `target`, enforcing the byte budget.
/// When `clamp_to_source` is set the budget is additionally clamped by the
/// encoded source length, so a tampered pack can't widen its own budget past
/// the original string's span (string / segment targets; fixed-width fields
/// carry their own hard bound instead). `None` = a diagnostic was recorded.
fn encode_checked(
    entry: &Entry,
    target: Target,
    clamp_to_source: bool,
    report: &mut ImportReport,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let source = match markup::encode(&entry.source, target) {
        Ok(b) => b,
        Err(issues) => {
            report.issue(
                &entry.key,
                format!(
                    "pack source doesn't encode (corrupted pack?): {}",
                    issues[0]
                ),
            );
            return None;
        }
    };
    let translated = match markup::encode(&entry.translation, target) {
        Ok(b) => b,
        Err(issues) => {
            let detail: Vec<String> = issues.iter().map(ToString::to_string).collect();
            report.issue(
                &entry.key,
                format!("translation not encodable: {}", detail.join("; ")),
            );
            return None;
        }
    };
    let budget = if clamp_to_source {
        entry.budget.min(source.len())
    } else {
        entry.budget
    };
    if translated.len() > budget {
        report.issue(
            &entry.key,
            format!(
                "translation needs {} bytes but the in-place budget is {budget} \
                 (shorten the text)",
                translated.len()
            ),
        );
        return None;
    }
    Some((source, translated))
}

/// `translated`, space-padded to exactly `len` bytes (dialog-segment form).
fn pad_segment(translated: &[u8], len: usize) -> Vec<u8> {
    let mut v = translated.to_vec();
    v.resize(len, 0x20);
    v
}

/// Plan one SCUS-string write: `(file_offset, bytes)`, or `None` when the
/// entry was resolved without a write (diagnostic / already applied).
fn plan_scus_str(
    scus: &[u8],
    entry: &Entry,
    va: u32,
    report: &mut ImportReport,
) -> Option<(usize, Vec<u8>)> {
    let (source, translated) = encode_checked(entry, Target::CString, true, report)?;
    let Some(off) = item_names::file_offset_for_va(scus, va) else {
        report.issue(&entry.key, "VA not in the SCUS data segment");
        return None;
    };
    let span = source.len().max(translated.len()) + 1;
    let Some(current) = scus.get(off..off + span) else {
        report.issue(&entry.key, "string span past end of SCUS");
        return None;
    };
    let cur_len = current
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(current.len());
    let cur = &current[..cur_len];
    if cur == translated.as_slice() {
        report.already_applied += 1;
        return None;
    }
    if cur != source.as_slice() {
        report.issue(
            &entry.key,
            "disc bytes don't match the pack source (different disc revision or \
             a conflicting patch) - skipped",
        );
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
    let (source, translated) = encode_checked(entry, Target::CString, false, report)?;
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
        return None;
    }
    if &field[..cur_len] != source.as_slice() {
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

/// One decompressed-domain segment edit, verified against the MAN bytes.
fn apply_segment_edit(
    buf: &mut [u8],
    entry: &Entry,
    off: usize,
    source: &[u8],
    translated: &[u8],
    report: &mut ImportReport,
) -> bool {
    let len = source.len();
    let in_range = off + len < buf.len();
    if !in_range || buf.get(off + len) != Some(&0x00) {
        report.issue(
            &entry.key,
            "segment framing not found at the keyed offset - skipped",
        );
        return false;
    }
    let cur = &buf[off..off + len];
    let padded = pad_segment(translated, len);
    if cur == padded.as_slice() {
        report.already_applied += 1;
        return false;
    }
    if cur != source {
        report.issue(
            &entry.key,
            "disc bytes don't match the pack source (different disc revision or \
             a conflicting patch) - skipped",
        );
        return false;
    }
    buf[off..off + len].copy_from_slice(&padded);
    true
}

/// Apply `pack` to the patcher's image. Untranslated entries are untouched.
pub fn import_pack(patcher: &mut DiscPatcher, pack: &LanguagePack) -> Result<ImportReport> {
    let mut report = ImportReport::default();

    // Group the work by write mechanism.
    let mut scus_work: Vec<&Entry> = Vec::new();
    let mut man_work: BTreeMap<usize, Vec<(usize, &Entry)>> = BTreeMap::new();
    let mut raw_work: Vec<(usize, usize, &Entry)> = Vec::new();
    for (_, entries) in pack.sections.iter() {
        for e in entries {
            if e.translation.trim().is_empty() {
                report.untranslated += 1;
                continue;
            }
            match parse_key(&e.key) {
                Some(Key::ScusStr { .. }) | Some(Key::ScusParty { .. }) => scus_work.push(e),
                Some(Key::Man { entry, off }) => {
                    man_work.entry(entry).or_default().push((off, e));
                }
                Some(Key::Raw { entry, off }) => raw_work.push((entry, off, e)),
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
        let mut wrote = 0usize;
        for (off, en) in &edits {
            let Some((source, translated)) = encode_checked(en, Target::Segment, true, &mut report)
            else {
                continue;
            };
            if apply_segment_edit(
                &mut man.decoded,
                en,
                *off,
                &source,
                &translated,
                &mut report,
            ) {
                wrote += 1;
            }
        }
        if wrote == 0 {
            continue;
        }
        match man.repack() {
            Some(stream) => {
                patcher.patch_prot_entry(entry_idx, man.man_offset as u64, &stream)?;
                report.applied += wrote;
            }
            None => {
                for (_, en) in &edits {
                    report.issue(
                        &en.key,
                        format!(
                            "scene {entry_idx}: recompressed MAN overflows its {} byte \
                             footprint - the whole scene was skipped (shorten these \
                             translations)",
                            man.compressed_budget
                        ),
                    );
                }
            }
        }
    }

    // Raw carriers: direct same-size in-place writes.
    for (entry_idx, off, en) in raw_work {
        let Some((source, translated)) = encode_checked(en, Target::Segment, true, &mut report)
        else {
            continue;
        };
        let mut window = match patcher.read_entry(entry_idx) {
            Ok(b) => b,
            Err(e) => {
                report.issue(&en.key, format!("PROT entry unreadable: {e}"));
                continue;
            }
        };
        if apply_segment_edit(&mut window, en, off, &source, &translated, &mut report) {
            patcher.patch_prot_entry(entry_idx, off as u64, &window[off..off + source.len()])?;
            report.applied += 1;
        }
    }

    Ok(report)
}
