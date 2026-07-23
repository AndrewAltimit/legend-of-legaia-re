//! Earth Egg coin-threshold edit + read-only listing.

use super::*;

use crate::earth_egg::{self, EarthEggInfo};

/// Outcome of an Earth Egg price edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EarthEggReport {
    /// PROT entry index of the scene bundle that was (or would be) edited.
    pub entry_idx: usize,
    /// Prior coins-required.
    pub old_price: u32,
    /// New coins-required.
    pub new_price: u32,
    /// Whether the MAN was actually rewritten (`false` = already at the value).
    pub changed: bool,
}

/// Read-only: locate the Earth Egg exchange on the disc and report its current
/// coins-required / threshold / debit. `None` if the disc doesn't carry it.
pub fn current_earth_egg(patcher: &DiscPatcher) -> Result<Option<EarthEggInfo>> {
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        if let Some(info) = earth_egg::list_price(&entry, idx) {
            return Ok(Some(info));
        }
    }
    Ok(None)
}

/// Set the Earth Egg coin threshold: require `new_price` coins to redeem it, and
/// debit exactly `new_price` on purchase (retail invariant gate = price - 1,
/// debit = price). Scans for the scene bundle carrying the bespoke scripted
/// exchange, recompresses its MAN in place, and writes it back EDC/ECC-valid.
///
/// Errors if the exchange isn't found, the price is out of range, or the
/// recompressed MAN would overflow its footprint. A price equal to the current
/// one is a successful no-op (`changed = false`).
pub fn set_earth_egg_price(patcher: &mut DiscPatcher, new_price: u32) -> Result<EarthEggReport> {
    earth_egg::validate_price(new_price)?;

    // Find the entry carrying the exchange.
    let mut located = None;
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        if let Some(info) = earth_egg::list_price(&entry, idx) {
            located = Some((idx, entry, info));
            break;
        }
    }
    let Some((idx, entry, info)) = located else {
        anyhow::bail!("Earth Egg exchange not found on this disc (no koin1 scene bundle?)");
    };

    match earth_egg::plan_set_price(&entry, idx, new_price)? {
        None => Ok(EarthEggReport {
            entry_idx: idx,
            old_price: info.price,
            new_price,
            changed: false,
        }),
        Some(edit) => {
            let stream = edit.exchange.repack().ok_or_else(|| {
                anyhow::anyhow!(
                    "recompressed koin1 MAN would overflow its footprint (Earth Egg price edit)"
                )
            })?;
            patcher
                .patch_prot_entry(idx, edit.exchange.man_offset as u64, &stream)
                .with_context(|| format!("write Earth Egg scene MAN (PROT entry {idx})"))?;
            Ok(EarthEggReport {
                entry_idx: idx,
                old_price: edit.old_price,
                new_price: edit.new_price,
                changed: true,
            })
        }
    }
}
