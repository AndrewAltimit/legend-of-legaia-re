//! Delilas Challenge - apply the Muscle Dome enrollment mod to a disc.

use super::*;

use legaia_asset::scene_asset_table::encode_size_word;

use crate::delilas_challenge::{DelilasRewards, DelilasSites};

/// Outcome of a Delilas Challenge application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelilasChallengeReport {
    /// PROT entry index of the `koin1` scene bundle that was edited.
    pub entry_idx: usize,
    /// Bytes the decompressed MAN grew by (`0` when already applied).
    pub grown_bytes: usize,
    /// Recompressed stream length.
    pub compressed_len: usize,
    /// Footprint the stream had to fit within.
    pub compressed_budget: usize,
    /// Whether the MAN was rewritten (`false` = the challenge was already
    /// present, a successful no-op).
    pub changed: bool,
}

/// Install the **Delilas Challenge** with the default prizes (3x Honey for a
/// solo win, 1x for a group win). See [`apply_delilas_challenge_with_rewards`].
pub fn apply_delilas_challenge(patcher: &mut DiscPatcher) -> Result<DelilasChallengeReport> {
    apply_delilas_challenge_with_rewards(patcher, DelilasRewards::default())
}

/// Install the **Delilas Challenge** on the Muscle Dome enrollment menu: a
/// fourth "who will enter" option offering a solo (1v3) or full-party (3v3)
/// fight against all three Delilas siblings at once, gated on the Koru death
/// event (story flag `0x378` - the `nilboa2` switch). Losing returns to the
/// venue (no game over); winning grants `rewards`. Pure script + formation
/// data edit inside the `koin1` scene bundle; see
/// [`crate::delilas_challenge`] for the byte-level design.
pub fn apply_delilas_challenge_with_rewards(
    patcher: &mut DiscPatcher,
    rewards: DelilasRewards,
) -> Result<DelilasChallengeReport> {
    // Resolve the koin1 block through CDNAME (raw #define numbers are raw-TOC
    // indices; extraction = raw - 2) and locate the enrollment script inside
    // it - the same resolution shape `apply_starting_bag` uses for town01.
    let cdname = patcher
        .cdname()
        .context("read CDNAME.TXT for the Muscle Dome scene")?;
    let (raw_start, raw_end) = legaia_prot::cdname::block_range_for_name(&cdname, "koin1")
        .context("CDNAME.TXT has no koin1 block")?;
    let ext_start =
        (raw_start as i64 - legaia_prot::cdname::RAW_TOC_INDEX_OFFSET as i64).max(0) as usize;
    let ext_end =
        (raw_end as i64 - legaia_prot::cdname::RAW_TOC_INDEX_OFFSET as i64).max(0) as usize;

    let mut located = None;
    for ext in ext_start..ext_end.min(patcher.entry_count()) {
        let entry = patcher
            .read_entry(ext)
            .with_context(|| format!("read PROT entry {ext}"))?;
        if let Some(sites) = DelilasSites::locate(&entry, ext) {
            located = Some(sites);
            break;
        }
    }
    let Some(sites) = located else {
        anyhow::bail!("koin1 block carries no Muscle Dome enrollment script");
    };
    let entry_idx = sites.entry_idx;

    if sites.already_applied {
        return Ok(DelilasChallengeReport {
            entry_idx,
            grown_bytes: 0,
            compressed_len: 0,
            compressed_budget: sites.compressed_budget,
            changed: false,
        });
    }

    let (new_man, grown) = sites
        .build(&rewards)
        .map_err(|e| anyhow::anyhow!("delilas-challenge build: {e}"))?;

    // Recompress under the descriptor boundary (greedy, then the optimal
    // packer - the koin1 MAN is sector-aligned with zero slack, and only the
    // optimal packer fits the grown script back in).
    let Some(stream) = crate::compress_within(&new_man, sites.compressed_budget) else {
        anyhow::bail!(
            "recompressed koin1 MAN overflows its {}-byte footprint",
            sites.compressed_budget
        );
    };

    patcher
        .patch_prot_entry(
            entry_idx,
            sites.man_descriptor_off as u64,
            &encode_size_word(0x03, new_man.len() as u32).to_le_bytes(),
        )
        .context("write koin1 MAN size word")?;
    patcher
        .patch_prot_entry(entry_idx, sites.man_offset as u64, &stream)
        .context("write koin1 MAN stream")?;

    Ok(DelilasChallengeReport {
        entry_idx,
        grown_bytes: grown,
        compressed_len: stream.len(),
        compressed_budget: sites.compressed_budget,
        changed: true,
    })
}
