//! Location / place-name renaming across all three carriers.
//!
//! See [`crate::location_name`] for the three sites and what each edit costs.
//! This module is the orchestration: resolve what the user asked to rename,
//! plan every carrier's edit against pristine bytes, then write.

use super::*;

use std::collections::BTreeMap;

use legaia_asset::worldmap_menu::NAME_COUNT;

use crate::location_name::{self, ManPlaceNames};

/// What to rename. The CLI / browser form accepts either form on the left of
/// an `=`: a bare number is a landmark index, anything else is the current
/// name (which is how the 14 places that have a world-map label but no
/// quick-travel cell - "Hunter's Spring", "Snowdrift Cave", "Sol Tower", ... -
/// are addressable at all).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameTarget {
    /// A `SCUS_942.54` landmark cell index (`0..16`).
    Index(usize),
    /// The place's current name, matched exactly.
    Name(String),
}

impl RenameTarget {
    /// Parse the left-hand side of a `TARGET=NAME` pair.
    pub fn parse(spec: &str) -> Self {
        let spec = spec.trim();
        match spec.parse::<usize>() {
            Ok(index) => Self::Index(index),
            Err(_) => Self::Name(spec.to_string()),
        }
    }
}

/// One requested rename, resolved to the old name every carrier is matched on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRename {
    /// The `SCUS_942.54` landmark cells this rename rewrites: the one the
    /// target named, or every cell carrying the target name.
    pub cells: Vec<usize>,
    /// The name being replaced.
    pub old_name: String,
    /// The replacement.
    pub new_name: String,
}

/// Outcome of a landmark/location rename.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocationRenameReport {
    /// Per rename: `(index, old_name, new_name)` for every `SCUS_942.54`
    /// landmark cell that changed (site 1).
    pub renames: Vec<(usize, String, String)>,
    /// World-map location records rewritten (site 2), across every kingdom
    /// MAN that carries the place.
    pub world_map_records: usize,
    /// Scene banner names rewritten (site 3).
    pub scene_banners: usize,
    /// PROT entries whose MAN was re-packed and written back.
    pub entries_changed: Vec<usize>,
    /// PROT entries that carry a matching name but whose re-packed MAN would
    /// not fit its footprint, so they were left vanilla. Empty in practice -
    /// every scene MAN is the last asset in its bundle and has its sector
    /// padding to grow into - but a skip is reported, never silently taken.
    pub skipped: Vec<usize>,
    /// Requested renames that matched nothing anywhere (an unknown name).
    pub unmatched: Vec<String>,
}

impl LocationRenameReport {
    /// `true` when no carrier changed.
    pub fn is_empty(&self) -> bool {
        self.renames.is_empty() && self.world_map_records == 0 && self.scene_banners == 0
    }
}

/// Rename one or more places **everywhere the disc shows the name**: the
/// `SCUS_942.54` quick-travel cell, the world-map label records in all three
/// kingdom MANs, and every scene MAN whose entry banner carries it.
///
/// Matching is by exact current name, so near-misses stay put: renaming
/// "Conkram" does not touch "Conkram (Past)", and renaming the "Sol"
/// quick-travel cell does not touch the "Sol Tower" scenes.
///
/// Every edit is planned against pristine bytes before the first write, so a
/// refused name (empty, non-ASCII, or past [`location_name::MAX_NAME_LEN`])
/// aborts with the disc untouched. A rename to the name a carrier already has
/// is a no-op for that carrier.
///
/// Ordering: this resizes scene MANs, so run it in the same slot as the door
/// randomizer - **after** a language pack, whose dialog edits are keyed by
/// byte offsets into the same buffers.
pub fn rename_locations_by_target(
    patcher: &mut DiscPatcher,
    renames: &[(RenameTarget, String)],
) -> Result<LocationRenameReport> {
    let mut report = LocationRenameReport::default();
    if renames.is_empty() {
        return Ok(report);
    }

    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for location rename")?;
    let cells = location_name::list_names(&scus)?;

    // 1. Resolve each request to the old name every carrier is matched on, and
    //    validate the replacement, before anything is written.
    let mut resolved: Vec<ResolvedRename> = Vec::with_capacity(renames.len());
    for (target, new_name) in renames {
        location_name::validate_name(new_name)?;
        match target {
            RenameTarget::Index(index) => {
                if *index >= NAME_COUNT {
                    anyhow::bail!("landmark index {index} out of range (0..{NAME_COUNT})");
                }
                resolved.push(ResolvedRename {
                    cells: vec![*index],
                    old_name: cells[*index].1.clone(),
                    new_name: new_name.clone(),
                });
            }
            RenameTarget::Name(old_name) => {
                resolved.push(ResolvedRename {
                    cells: cells
                        .iter()
                        .filter(|(_, n)| n == old_name)
                        .map(|(i, _)| *i)
                        .collect(),
                    old_name: old_name.clone(),
                    new_name: new_name.clone(),
                });
            }
        }
    }

    // 2. Site 1 - the SCUS landmark cells. A name-keyed request rewrites every
    //    cell that carries the name; an index-keyed one rewrites just that cell.
    let mut scus_edits = Vec::new();
    for r in &resolved {
        for &i in &r.cells {
            if let Some(edit) = location_name::plan_rename(&scus, i, &r.new_name)? {
                scus_edits.push(edit);
            }
        }
    }

    // 3. Sites 2 + 3 - every scene bundle's MAN. One sweep applies all the
    //    renames a MAN matches, so a bundle is decompressed and re-packed once.
    let mut man_writes: Vec<(ManPlaceNames, Vec<u8>, u32)> = Vec::new();
    // A request whose new name already *is* the old one is an idempotent
    // no-op, not a miss - seed it as matched so it never reads as unknown.
    let mut matched: BTreeMap<String, bool> = resolved
        .iter()
        .map(|r| (r.old_name.clone(), r.old_name == r.new_name))
        .collect();
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        let footprint = patcher
            .entry_true_footprint_sectors(idx)
            .map(|s| s as usize * 2048)
            .unwrap_or(entry.len());
        let Some(mut carrier) = ManPlaceNames::locate(&entry, idx, footprint) else {
            continue;
        };
        let mut touched = false;
        for r in &resolved {
            if r.old_name == r.new_name {
                continue;
            }
            let counts = carrier
                .rename(&r.old_name, &r.new_name)
                .map_err(|e| anyhow::anyhow!("rename {:?} in PROT entry {idx}: {e}", r.old_name))?;
            if !counts.is_empty() {
                matched.insert(r.old_name.clone(), true);
                report.world_map_records += counts.world_map_records;
                report.scene_banners += counts.scene_banners;
                touched = true;
            }
        }
        if !touched {
            continue;
        }
        match carrier.repack() {
            Some((stream, size)) => man_writes.push((carrier, stream, size)),
            None => report.skipped.push(idx),
        }
    }

    // 4. Write. SCUS first (it can't fail once planned), then each MAN's size
    //    word and stream.
    for edit in scus_edits {
        patcher
            .patch_named_file(SCUS_NAME, edit.offset as u64, &edit.slot)
            .with_context(|| format!("write location name {} ({:?})", edit.index, edit.new_name))?;
        matched.insert(edit.old_name.clone(), true);
        report
            .renames
            .push((edit.index, edit.old_name, edit.new_name));
    }
    for (carrier, stream, size) in &man_writes {
        patcher
            .patch_prot_entry(
                carrier.entry_idx,
                carrier.man_descriptor_off as u64,
                &legaia_asset::scene_asset_table::encode_size_word(0x03, *size).to_le_bytes(),
            )
            .with_context(|| format!("write MAN size word (PROT entry {})", carrier.entry_idx))?;
        patcher
            .patch_prot_entry(carrier.entry_idx, carrier.man_offset as u64, stream)
            .with_context(|| format!("write scene MAN (PROT entry {})", carrier.entry_idx))?;
        report.entries_changed.push(carrier.entry_idx);
    }
    report.entries_changed.sort_unstable();
    report.skipped.sort_unstable();
    report.unmatched = matched
        .into_iter()
        .filter(|(_, hit)| !hit)
        .map(|(name, _)| name)
        .collect();
    Ok(report)
}

/// Index-keyed convenience wrapper over [`rename_locations_by_target`], kept
/// for callers that only address the 16 quick-travel cells.
pub fn rename_locations(
    patcher: &mut DiscPatcher,
    renames: &[(usize, String)],
) -> Result<LocationRenameReport> {
    let targets: Vec<(RenameTarget, String)> = renames
        .iter()
        .map(|(i, n)| (RenameTarget::Index(*i), n.clone()))
        .collect();
    rename_locations_by_target(patcher, &targets)
}

/// Read-only: every place name the disc carries, per site. Used by the
/// `locations` listing so a user can see what is addressable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocationInventory {
    /// The 16 `SCUS_942.54` quick-travel cells, in index order.
    pub landmarks: Vec<(usize, String)>,
    /// The world-map label records (region, x, y, name), from the first
    /// kingdom MAN that carries the table - all three carry the same one.
    pub world_map: Vec<(u8, u8, u8, String)>,
    /// Scene banner names -> how many scene bundles carry each.
    pub scene_banners: BTreeMap<String, usize>,
}

/// The world-map label records `(region, map_x, map_y, name)`, off the first
/// kingdom MAN that carries the table - all three carry the same one, so the
/// sweep stops at the first hit rather than decompressing all ~90 scene MANs
/// like [`list_locations`] does. This is the cheap listing the browser
/// patcher runs when the user picks a disc.
pub fn list_world_map_labels(patcher: &DiscPatcher) -> Vec<(u8, u8, u8, String)> {
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        let footprint = patcher
            .entry_true_footprint_sectors(idx)
            .map(|s| s as usize * 2048)
            .unwrap_or(entry.len());
        let Some(carrier) = ManPlaceNames::locate(&entry, idx, footprint) else {
            continue;
        };
        if let Some(table) = carrier.world_map.as_ref() {
            return table
                .locations
                .iter()
                .map(|l| (l.region, l.map_x, l.map_y, l.name.clone()))
                .collect();
        }
    }
    Vec::new()
}

/// Collect [`LocationInventory`] off a disc.
pub fn list_locations(patcher: &DiscPatcher) -> Result<LocationInventory> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for location listing")?;
    let mut inv = LocationInventory {
        landmarks: location_name::list_names(&scus)?,
        ..Default::default()
    };
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        let footprint = patcher
            .entry_true_footprint_sectors(idx)
            .map(|s| s as usize * 2048)
            .unwrap_or(entry.len());
        let Some(carrier) = ManPlaceNames::locate(&entry, idx, footprint) else {
            continue;
        };
        if let Some(name) = carrier.scene_name.as_ref() {
            *inv.scene_banners.entry(name.name.clone()).or_default() += 1;
        }
        if inv.world_map.is_empty()
            && let Some(table) = carrier.world_map.as_ref()
        {
            inv.world_map = table
                .locations
                .iter()
                .map(|l| (l.region, l.map_x, l.map_y, l.name.clone()))
                .collect();
        }
    }
    Ok(inv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_parses_index_or_name() {
        assert_eq!(RenameTarget::parse(" 3 "), RenameTarget::Index(3));
        assert_eq!(
            RenameTarget::parse("Hunter's Spring"),
            RenameTarget::Name("Hunter's Spring".into())
        );
        // A name that merely starts with a digit is still a name.
        assert_eq!(
            RenameTarget::parse("2nd Camp"),
            RenameTarget::Name("2nd Camp".into())
        );
    }

    #[test]
    fn empty_report_reads_empty() {
        assert!(LocationRenameReport::default().is_empty());
    }
}
