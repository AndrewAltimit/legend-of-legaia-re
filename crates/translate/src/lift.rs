//! Lift the text of another Latin-script Legaia disc into a **USA-keyed
//! working pack**.
//!
//! Every Latin-script release is 1:1 with the USA disc at the container level
//! (see `docs/tooling/pal-localizations.md`): a USA PROT coordinate names the
//! same logical asset on every disc, the five SCUS name tables exist id-for-id
//! at build-shifted VAs, and the `0x1F`-segment dialog corpus pairs by position
//! within each PROT entry. This module re-keys the source disc's text onto the
//! USA coordinate space the [importer](super::import) patches. The source may be
//!
//! - one of the three **measured** official PAL localizations
//!   (`SCES_019.44`/`.45`/`.46` = FR/DE/IT), whose name-table bases are pinned;
//! - an **unmeasured** official build (`SCES_019.47` Spain, `SCES_017.52` EU
//!   English), whose bases are located by scanning out from the USA VAs;
//! - a **fan-patched** disc of any of the above - including a patched USA disc
//!   (`SCUS_942.54`) - which is how a community translation shipped as a binary
//!   patch becomes an editable pack: patch the disc it was built for, then lift.
//!
//! The Japanese builds are not lifted: their text is not the Latin codec.
//!
//! - **Name tables** (item / spell / arts / accessory / party): id-for-id. The
//!   USA pack keys each pooled string by its *USA* virtual address; the same id
//!   on the source exe points at the localized string, so the map is
//!   `usa_string_va -> source_string`. The source base is *located* (a pinned
//!   VA is verified by following its pointers, with a windowed search fallback;
//!   an unpinned build is searched from the USA VA outright), never trusted
//!   blind.
//! - **Dialog** (`man:` scene-bundle MANs, `raw:` streaming-scene MANs):
//!   structural. Each MAN is walked record by record with the field-VM
//!   disassembler and a line is keyed `(record ordinal, ordinal among that
//!   record's text leads)` - the same coordinate on every build, because the
//!   script is one program with different strings (byte offsets differ - the
//!   localized MAN repacks). A line the walk does not place falls back to the
//!   scan ordinal (the Nth qualifying segment of the entry), which is what a
//!   line the other disc's quality gate drops would otherwise shift.
//! - **String pools** (`ui:` overlay pools, `system_text` SCUS strings,
//!   `place_names` cells): NUL-chunk lists of the same window on both builds,
//!   anchored on byte-identical chunks and paired by ordinal inside each
//!   partition ([`pair_chunk_lists`]).
//!
//! The result is a **working pack** (`source:` = USA text, `translation:` =
//! official PAL text) carrying the USA per-string byte budgets. It is filled
//! with the game's copyrighted text, so it is scratchpad-only output - never
//! committed. The lifted `translation` bytes are the raw PAL bytes decoded to
//! markup (accents become `{82}`-style single-byte escapes), which the
//! [markup codec](super::markup) round-trips exactly.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

use legaia_game_tables::{item_names, new_game, spell_names};

use legaia_disc_patch::disc::DiscPatcher;

use super::export::{SceneManText, export_pack};
use super::pack::LanguagePack;
use super::{markup, segments};

mod chunk_align;
mod dialog;
mod source_tables;
mod string_pools;

use dialog::{DialogDomain, fill_dialog, man_of_scene, man_of_stream};
pub use dialog::{drop_baseline_text, fold_pack_accents};
pub use source_tables::{
    LocatedBases, SourceBuild, locate_unpinned, pinned_bases_for_exe, source_build_for_exe,
};
use source_tables::{
    SourceBases, TABLES, locate_base, locate_party_base, looks_like_name, read_cstr, read_ptr,
};
use string_pools::{
    lift_monster_names, lift_place_cells, lift_scus_pools, lift_ui_pools, pool_ref_pairs,
};

/// Per-table outcome for the lift report.
#[derive(Debug, Clone)]
pub struct TableStat {
    pub name: &'static str,
    pub located: bool,
    pub pal_base: u32,
    pub valid_fraction: f64,
    /// USA string VAs mapped to a PAL string (map insertions this table made).
    pub paired: usize,
}

/// How one dialog domain's lines were paired (counts only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PairStats {
    /// Lines paired **structurally**: the same `(record, ordinal)` on both
    /// discs' clean script walks ([`walk_texts`]).
    pub structural: usize,
    /// Lines the walk does not reach on the USA side (operand runs, text
    /// past a record's first decode error) or whose entry has a different
    /// record shape on the source disc, paired by scan ordinal instead.
    pub positional: usize,
    /// Of the structural pairs, how many the scan-ordinal pairing would have
    /// matched to a **different** source line - the lines an added, removed
    /// or gate-failing segment earlier in the same entry shifted.
    pub shifted: usize,
}

/// Whole-lift outcome (counts only - no text).
#[derive(Debug, Clone, Default)]
pub struct LiftReport {
    pub language: String,
    pub exe_name: String,
    /// Human label of the source build (`France (PAL)`, `USA`, ...).
    pub build_label: String,
    pub tables: Vec<TableStat>,
    /// Where the party template was read on the source exe, if anywhere.
    pub party_base: Option<u32>,
    /// Whether the template's stat bytes matched the USA fingerprint there.
    pub party_fingerprint_ok: bool,
    /// `scus:str:*` pack entries filled / left empty (no PAL string mapped).
    pub names_filled: usize,
    pub names_unmapped: usize,
    /// `scus:party:*` entries filled.
    pub party_filled: usize,
    pub party_total: usize,
    /// `man:` dialog: pack entries, order-paired (filled), unpaired.
    pub man_total: usize,
    pub man_paired: usize,
    /// `raw:` carriers: same three.
    pub raw_total: usize,
    pub raw_paired: usize,
    /// How the `man:` / `raw:` pairs were made.
    pub man_pairing: PairStats,
    pub raw_pairing: PairStats,
    /// Overlay `ui:` pool strings paired / total, and per pool.
    pub ui_paired: usize,
    pub ui_total: usize,
    pub ui_pools: Vec<PoolStat>,
    /// `system_text` SCUS strings paired / total.
    pub system_paired: usize,
    pub system_total: usize,
    /// World-map place-name cells paired / total.
    pub cells_paired: usize,
    pub cells_total: usize,
    /// Monster names paired / total.
    pub monsters_paired: usize,
    pub monsters_total: usize,
    /// Pool strings paired through a code / data reference (the rest pair by
    /// the pool's chunk list).
    pub pool_ref_paired: usize,
}

/// One string pool's pairing outcome.
#[derive(Debug, Clone)]
pub struct PoolStat {
    pub label: String,
    pub paired: usize,
    pub total: usize,
}

impl LiftReport {
    pub fn man_unpaired(&self) -> usize {
        self.man_total - self.man_paired
    }
    pub fn raw_unpaired(&self) -> usize {
        self.raw_total - self.raw_paired
    }
}

/// MAN-domain segment texts of a PROT entry, in the exact scan order the pack
/// keys follow. `allow_high` widens the gate for a PAL build's accented lines.
fn man_seg_texts(entry: &[u8], allow_high: bool) -> Vec<(usize, Vec<u8>)> {
    match SceneManText::locate(entry) {
        Some(man) => segments::scan_man(&man.decoded, allow_high)
            .iter()
            .map(|s| {
                (
                    s.text_off,
                    man.decoded[s.text_off..s.text_off + s.len].to_vec(),
                )
            })
            .collect(),
        None => Vec::new(),
    }
}

/// Raw-carrier segment texts of a PROT entry (mirrors [`super::export`]): gated
/// on the dialog-carrier check, skipping anything inside the compressed MAN.
fn raw_seg_texts(entry: &[u8], allow_high: bool) -> Vec<(usize, Vec<u8>)> {
    segments::scan_raw_carrier(entry, allow_high)
        .iter()
        .map(|s| (s.text_off, entry[s.text_off..s.text_off + s.len].to_vec()))
        .collect()
}

/// Parse the PROT entry index out of a `man:<idx>:0x..` / `raw:<idx>:0x..` key.
fn key_entry_index(key: &str) -> Option<usize> {
    let mut it = key.split(':');
    let _kind = it.next()?;
    it.next()?.parse().ok()
}

/// Parse the `0x<va>` of a `scus:str:0x<va>` key.
fn key_scus_va(key: &str) -> Option<u32> {
    let hex = key.strip_prefix("scus:str:0x")?;
    u32::from_str_radix(hex, 16).ok()
}

/// Parse the roster slot of a `scus:party:<n>` key.
fn key_party_slot(key: &str) -> Option<usize> {
    key.strip_prefix("scus:party:")?.parse().ok()
}

/// Lift the text on `source` onto `target`'s coordinate space. Returns a
/// filled working pack + a counts-only report. The pack's language is the
/// build's default ([`SourceBuild::lang`]); a fan translation's caller restamps
/// it (`pack.language`) - nothing in the exe name says what language a patch
/// carries.
pub fn lift_official(
    target: &DiscPatcher,
    source: &DiscPatcher,
) -> Result<(LanguagePack, LiftReport)> {
    let exe_name = legaia_disc_patch::disc::boot_exe_name(source)?;
    let Some(build) = source_build_for_exe(&exe_name) else {
        bail!(
            "source boot exe {exe_name:?} is not a Latin-script Legaia build this lift can read \
             (known: SCUS_942.54, SCES_017.52, SCES_019.44/.45/.46/.47; the JP discs use a \
             different text encoding)"
        );
    };
    let lang = build.lang;
    let (pinned_tables, pinned_party) = match build.bases {
        SourceBases::Pinned(p) => (p.table_bases.map(Some), Some(p.party_base)),
        SourceBases::Located => ([None; 4], None),
    };

    // Start from the USA source pack: correct keys, budgets, and `source` text.
    let mut pack = export_pack(target)?;
    pack.language = lang.to_string();
    pack.notes = format!(
        "Text lifted from {exe_name} ({label}) onto USA coordinates (translate \
         lift-official). Contains the game's text - scratchpad only, never commit.",
        label = build.label
    );

    let mut report = LiftReport {
        language: lang.to_string(),
        exe_name: exe_name.clone(),
        build_label: build.label.to_string(),
        ..Default::default()
    };

    // ---- Name tables: build usa_string_va -> pal_string ----
    let usa_exe = target
        .read_named_file("SCUS_942.54")
        .context("SCUS_942.54 not found on target disc")?;
    let pal_exe = source
        .read_named_file(&exe_name)
        .with_context(|| format!("{exe_name} not found on source disc"))?;

    let mut str_map: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for (spec, &pinned) in TABLES.iter().zip(&pinned_tables) {
        // Validate/locate against every pointer field the table owns, then
        // the meta columns.
        let located = locate_base(&usa_exe, &pal_exe, spec, pinned);
        let mut stat = TableStat {
            name: spec.name,
            located: located.is_some(),
            pal_base: located.map(|(b, _)| b).or(pinned).unwrap_or(spec.usa_base),
            valid_fraction: located.map(|(_, f)| f).unwrap_or(0.0),
            paired: 0,
        };
        if let Some((pal_base, _)) = located {
            for id in 0..spec.count {
                for &field in spec.fields {
                    let Some(usa_str_va) =
                        read_ptr(&usa_exe, spec.usa_base + id * spec.stride + field)
                    else {
                        continue;
                    };
                    if usa_str_va == 0 {
                        continue;
                    }
                    let Some(pal_str_va) = read_ptr(&pal_exe, pal_base + id * spec.stride + field)
                    else {
                        continue;
                    };
                    if pal_str_va == 0 {
                        continue;
                    }
                    let Some(pal_str) = read_cstr(&pal_exe, pal_str_va) else {
                        continue;
                    };
                    if str_map.insert(usa_str_va, pal_str).is_none() {
                        stat.paired += 1;
                    }
                }
            }
        }
        report.tables.push(stat);
    }

    // Spell descriptions: the flat pointer table right after the spell
    // records (`spell_names::DESC_PTR_TABLE_VA`, `0xBE` records in on every
    // build), slot for slot from the located spell base.
    if let Some(stat) = report
        .tables
        .iter()
        .find(|t| t.name == "spells" && t.located)
    {
        let table_off = spell_names::DESC_PTR_TABLE_VA - 0x8007_54C8;
        for idx in 1..super::export::DESC_TABLE_SLOTS {
            let (Some(usa_str_va), Some(pal_str_va)) = (
                read_ptr(&usa_exe, spell_names::DESC_PTR_TABLE_VA + idx * 4),
                read_ptr(&pal_exe, stat.pal_base + table_off + idx * 4),
            ) else {
                continue;
            };
            if usa_str_va == 0 || pal_str_va == 0 {
                continue;
            }
            if let Some(pal_str) = read_cstr(&pal_exe, pal_str_va)
                && looks_like_name(&pal_str)
            {
                str_map.entry(usa_str_va).or_insert(pal_str);
            }
        }
    }

    // Fill the name-table `scus:str` entries from the map (the `system_text`
    // pools are SCUS strings too, but not pointer-addressed - they pair by
    // pool below).
    let sections = &mut pack.sections;
    for entries in [
        &mut sections.items,
        &mut sections.item_types,
        &mut sections.spells,
        &mut sections.arts,
        &mut sections.accessory_passives,
    ] {
        for e in entries.iter_mut() {
            let Some(va) = key_scus_va(&e.key) else {
                continue;
            };
            match str_map.get(&va) {
                Some(bytes) => {
                    e.translation = markup::decode(bytes);
                    report.names_filled += 1;
                }
                None => report.names_unmapped += 1,
            }
        }
    }

    // ---- Party names: fixed 10-byte fields, id-for-id ----
    report.party_total = pack.sections.party_names.len();
    let party = locate_party_base(&usa_exe, &pal_exe, pinned_party);
    report.party_base = party.map(|(b, _)| b);
    report.party_fingerprint_ok = party.is_some_and(|(_, ok)| ok);
    for e in pack.sections.party_names.iter_mut() {
        let Some((party_base, _)) = party else {
            break;
        };
        let Some(slot) = key_party_slot(&e.key) else {
            continue;
        };
        if slot >= new_game::PARTY_RECORDS {
            continue;
        }
        let va = party_base + (slot * new_game::RECORD_STRIDE) as u32 + 16;
        let Some(off) = item_names::file_offset_for_va(&pal_exe, va) else {
            continue;
        };
        let Some(field) = pal_exe.get(off..off + new_game::NAME_LEN) else {
            continue;
        };
        let len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        if len == 0 {
            continue;
        }
        e.translation = markup::decode(&field[..len]);
        report.party_filled += 1;
    }

    // ---- Monster names: record for record ----
    lift_monster_names(target, source, &mut pack, &mut report);

    // ---- Dialog: structural pairing per PROT entry ----
    // Group MAN / raw pack entries by PROT index (they are already in scan
    // order within a group, matching the USA scan the pack was built from).
    let man_stats = fill_dialog(
        target,
        source,
        &mut pack.sections.scene_dialog,
        DialogDomain {
            seg_texts: man_seg_texts,
            man_of: man_of_scene,
            keys_are_entry_offsets: false,
        },
        &mut report.man_total,
        &mut report.man_paired,
    )?;
    report.man_pairing = man_stats;
    let raw_stats = fill_dialog(
        target,
        source,
        &mut pack.sections.inline_text,
        DialogDomain {
            seg_texts: raw_seg_texts,
            man_of: man_of_stream,
            keys_are_entry_offsets: true,
        },
        &mut report.raw_total,
        &mut report.raw_paired,
    )?;
    report.raw_pairing = raw_stats;

    // ---- Overlay UI pools, SCUS system strings, place-name cells ----
    let by_ref = pool_ref_pairs(target, source, &usa_exe, &pal_exe, &exe_name, &pack);
    lift_ui_pools(target, source, &by_ref, &mut pack, &mut report);
    let drift = report
        .tables
        .iter()
        .zip(TABLES)
        .filter(|(t, _)| t.located)
        .map(|(t, spec)| t.pal_base as i64 - spec.usa_base as i64)
        .next_back()
        .unwrap_or(0);
    lift_scus_pools(&usa_exe, &pal_exe, drift, &by_ref, &mut pack, &mut report);
    lift_place_cells(&usa_exe, &pal_exe, drift, &mut pack, &mut report);

    // The source-side text is written to fit its own slots, padded with
    // trailing spaces where a translator kept a line same-size; those spaces
    // draw nothing and only cost budget on the target, so they are dropped.
    for entries in pack.sections.each_mut() {
        for e in entries.iter_mut() {
            let trimmed = e.translation.trim_end_matches(' ');
            if !trimmed.is_empty() && trimmed.len() != e.translation.len() {
                e.translation.truncate(trimmed.len());
            }
        }
    }

    Ok((pack, report))
}

#[cfg(test)]
mod tests;
