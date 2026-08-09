//! Delilas Challenge - apply the Muscle Dome enrollment mod to a disc.
//!
//! Two coordinated halves ship together (the koin1 warp is meaningless without
//! the arena course it warps into, and vice-versa):
//!
//! - [`inject_delilas_dome`] - the code-injection half: a 2-round dome
//!   *course* (Che & Lu double-team, then Gi) installed into the arena overlay
//!   (PROT 0977), a `SCUS_942.54` routine cave + stream-map hook, and the two
//!   slim-clone archive slots the double-team round streams (see
//!   [`crate::delilas_dome`]).
//! - the koin1 half: a fourth "who will enter" enrollment option that warps
//!   into the arena requesting that course (see [`crate::delilas_challenge`]).

use super::*;

use legaia_asset::monster_archive;
use legaia_asset::scene_asset_table::encode_size_word;

use crate::delilas_challenge::DelilasSites;
use crate::delilas_dome::{
    ARENA_BASE_VA, ARENA_OVERLAY_PROT_INDEX, BATTLE_OVERLAY_PROT_INDEX, CLONE_IDS,
    DELILAS_PAIR_IDS, DomeInjection, ROUTINE_VA, SEED_HOOK_VA,
};

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
    /// Whether the arena dome course was injected (`false` = already present).
    pub dome_injected: bool,
    /// Whether the MAN was rewritten (`false` = the challenge was already
    /// present, a successful no-op).
    pub changed: bool,
}

/// Build the two slim-clone archive slots for the double-team round: Che
/// (163) and Lu (164) minus their generic-AI castable spell entries -
/// except each sibling's probe-traced special-move choreography entries
/// (`delilas_dome::slim_policy`) - encoded as slots for [`CLONE_IDS`]
/// (190/191). The originals are read from - and never written back to -
/// the user's own disc; see `legaia_asset::monster_archive::slim_castables`
/// for what is dropped and why the fight cannot tell.
fn build_clone_slots(patcher: &DiscPatcher) -> Result<[(u16, Vec<u8>); 2]> {
    let mut out: Vec<(u16, Vec<u8>)> = Vec::with_capacity(2);
    for (&src, &dst) in DELILAS_PAIR_IDS.iter().zip(CLONE_IDS.iter()) {
        let slot = patcher
            .monster_slot(src)
            .with_context(|| format!("read monster slot {src}"))?;
        let size = u32::from_le_bytes(slot[..4].try_into().unwrap()) as usize;
        let block = legaia_lzs::decompress(&slot[4..], size)
            .with_context(|| format!("decode monster block {src}"))?;
        let (protected, extra_drop) = crate::delilas_dome::slim_policy(src);
        let slim = monster_archive::slim_castables(&block, protected, extra_drop)
            .with_context(|| format!("slim monster block {src}"))?;
        let encoded = monster_archive::encode_slot(&slim.bytes)
            .with_context(|| format!("encode slim clone slot {dst}"))?;
        out.push((dst, encoded));
    }
    Ok(out.try_into().expect("exactly two clone slots"))
}

/// Inject the **Delilas dome course** (the code-hook half): a 2-round Muscle
/// Dome course - Che & Lu together, then Gi - installed into the arena
/// overlay (PROT 0977), a `SCUS_942.54` routine cave + stream-map hook, and
/// the two slim-clone archive slots the double-team round streams. See
/// [`crate::delilas_dome`] for the byte-level design.
///
/// Idempotent: if the seed hook is already the injected `j ROUTINE_VA`, the
/// course is present and this is a no-op returning `false`. Fails (without
/// touching the disc) if the build isn't the recognized US layout.
pub fn inject_delilas_dome(patcher: &mut DiscPatcher) -> Result<bool> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for delilas-dome injection")?;
    let overlay = patcher
        .read_entry(ARENA_OVERLAY_PROT_INDEX)
        .context("read arena overlay (0977) for delilas-dome injection")?;
    let battle = patcher
        .read_entry(BATTLE_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay (0898) for delilas-dome injection")?;

    // Idempotency: once applied, the seed hook is our `j ROUTINE_VA`.
    let seed_off = (SEED_HOOK_VA - ARENA_BASE_VA) as usize;
    let cur = overlay
        .get(seed_off..seed_off + 4)
        .map(|w| u32::from_le_bytes(w.try_into().unwrap()));
    if cur == Some(crate::mips::j(ROUTINE_VA)) {
        return Ok(false);
    }

    // Plan everything before writing anything: the code plan validates every
    // hook fingerprint, and the clone slots must build from the disc's own
    // archive (they are read from the pristine 163/164 slots).
    let plan = DomeInjection::plan(&scus, &overlay, &battle)?;
    let clones = build_clone_slots(patcher)?;

    // Slim-clone archive slots first (data the hooks will stream), then the
    // SCUS-cave writes + stream hook, then the arena overlay detours.
    for (dst, slot) in &clones {
        patcher
            .patch_monster_slot(*dst, slot)
            .with_context(|| format!("write slim clone slot {dst}"))?;
    }
    for w in &plan.scus {
        patcher
            .patch_named_file(SCUS_NAME, w.off as u64, &w.bytes)
            .with_context(|| format!("write delilas-dome SCUS write at {:#x}", w.off))?;
    }
    for w in &plan.overlay {
        patcher
            .patch_prot_entry(ARENA_OVERLAY_PROT_INDEX, w.off as u64, &w.bytes)
            .with_context(|| format!("write delilas-dome overlay hook at {:#x}", w.off))?;
    }
    for w in &plan.battle {
        patcher
            .patch_prot_entry(BATTLE_OVERLAY_PROT_INDEX, w.off as u64, &w.bytes)
            .with_context(|| format!("write delilas-dome magic-reject mask at {:#x}", w.off))?;
    }
    Ok(true)
}

/// Install the **Delilas Challenge** on the Muscle Dome enrollment menu: a
/// fourth "who will enter" option that warps into the arena and runs the new
/// Delilas dome course (Che & Lu together, then Gi), gated on the Koru death event (story
/// flag `0x378` - the `nilboa2` switch). Losing a dome round returns to the
/// venue (no game over) by the dome's own design.
///
/// Installs both halves: the arena dome course ([`inject_delilas_dome`]) and
/// the koin1 menu + warp (a script edit inside the `koin1` scene bundle; see
/// [`crate::delilas_challenge`]). The koin1 MAN is built (and validated)
/// before either half is written, so an unbuildable edit aborts without
/// touching the disc.
pub fn apply_delilas_challenge(patcher: &mut DiscPatcher) -> Result<DelilasChallengeReport> {
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
        // The menu is already present; ensure the arena course is too (a
        // re-run over a partially-applied image), then report the no-op.
        let dome_injected = inject_delilas_dome(patcher)?;
        return Ok(DelilasChallengeReport {
            entry_idx,
            grown_bytes: 0,
            compressed_len: 0,
            compressed_budget: sites.compressed_budget,
            dome_injected,
            changed: false,
        });
    }

    // Build (and validate) the grown koin1 MAN first, then recompress under the
    // descriptor boundary (greedy, then the optimal packer - the koin1 MAN is
    // sector-aligned with zero slack, and only the optimal packer fits the
    // grown script back in). Nothing is written if either step fails.
    let (new_man, grown) = sites
        .build()
        .map_err(|e| anyhow::anyhow!("delilas-challenge build: {e}"))?;
    let Some(stream) = crate::compress_within(&new_man, sites.compressed_budget) else {
        anyhow::bail!(
            "recompressed koin1 MAN overflows its {}-byte footprint",
            sites.compressed_budget
        );
    };

    // Arena course first (the warp target), then the koin1 menu + warp.
    let dome_injected = inject_delilas_dome(patcher)?;

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
        dome_injected,
        changed: true,
    })
}
