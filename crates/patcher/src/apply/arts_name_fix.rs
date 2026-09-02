//! Arts name-length fix, by ZetaPhoenix. See [`crate::arts_name_fix`] for the
//! retail bug, the routine, and why the carrier relocates it into arena 1.

use super::*;

/// Outcome of installing the arts name-length fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtsNameFixReport {
    /// Where the routine was parked (arena 1; behind the Super Arts Pack's
    /// battle-load stub when installed together).
    pub routine_va: u32,
    /// Same-size edits written (the hook and the routine).
    pub edits: usize,
}

/// Install the **arts name-length fix by ZetaPhoenix**: the Super / Miracle
/// Art name banner is re-centred for the name actually installed, instead of
/// retail's stale "Vulture Blade" placeholder measurement. Shipped as part of
/// `--super-arts-pack` - the author's own update to his mod - with
/// `routine_va` = [`crate::super_arts_pack::ARENA_USED_END_VA`], directly
/// behind the pack's battle-load stub. (The plan accepts any arena-1 spot;
/// the oracles also exercise the arena head.) Fails without touching the disc
/// if the hook site is not the recognized US build or the region is not dead
/// space.
pub fn inject_arts_name_fix(
    patcher: &mut DiscPatcher,
    routine_va: u32,
) -> Result<ArtsNameFixReport> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for the arts name-length fix")?;
    let plan = crate::arts_name_fix::ArtsNameFixInjection::plan(&scus, routine_va)
        .context("arts name-length fix (by ZetaPhoenix)")?;
    for edit in &plan.edits {
        patcher
            .patch_named_file(SCUS_NAME, edit.file_off as u64, &edit.bytes)
            .with_context(|| format!("write arts-name-fix SCUS edit at {:#x}", edit.file_off))?;
    }
    Ok(ArtsNameFixReport {
        routine_va: plan.routine_va,
        edits: plan.edits.len(),
    })
}
