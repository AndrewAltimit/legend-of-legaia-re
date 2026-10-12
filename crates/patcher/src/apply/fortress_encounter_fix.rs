//! Fortress encounter fix. See [`crate::fortress_encounter_fix`] for the retail
//! defect, the table edit, and why it duplicates a group rather than
//! retargeting a flag.

use super::*;

use crate::fortress_encounter_fix::{self, FORTRESS_SCENES};

/// Outcome of applying the Fortress encounter fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FortressEncounterFixReport {
    /// Scene MANs rewritten (`0` when both were already fixed, else up to
    /// `2`: `deroa` and `chitei2`).
    pub scenes_changed: usize,
}

impl FortressEncounterFixReport {
    /// Whether anything was written (`false` = the disc already carried the fix).
    pub fn changed(&self) -> bool {
        self.scenes_changed != 0
    }
}

/// Apply the **Fortress encounter fix**: in both Absolute Fortress scenes
/// (`deroa`, `chitei2`) the leading flag-`0x15` encounter group - a copy of the
/// Floating Castle's "no battles in the escape's last minute" group, selected
/// for good on any save whose castle clock dipped under a minute - becomes a
/// duplicate of the scene's own post-Jette group, so the Fortress no longer
/// reads the stale flag. Seedless, same-size (14 decoded bytes per scene), and
/// re-packed into each MAN's own footprint.
///
/// Both scenes are planned and re-packed **before** either is written, so a
/// disc whose tables are not retail's (or this fix's own output), or a MAN that
/// would not fit back, fails with nothing changed. Idempotent: an
/// already-fixed scene is skipped.
pub fn apply_fortress_encounter_fix(
    patcher: &mut DiscPatcher,
) -> Result<FortressEncounterFixReport> {
    let mut writes: Vec<(usize, u64, Vec<u8>)> = Vec::new();
    for (idx, name) in FORTRESS_SCENES {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx} ({name})"))?;
        let mut scene = SceneEncounters::locate(&entry, idx).with_context(|| {
            format!("PROT entry {idx} is not the {name} scene bundle (no MAN encounter table)")
        })?;
        let Some(fixed) = fortress_encounter_fix::plan(&scene.decoded)
            .with_context(|| format!("{name} encounter table (PROT entry {idx})"))?
        else {
            continue;
        };
        scene.decoded = fixed;
        let stream = scene.repack().with_context(|| {
            format!("recompressed {name} MAN would overflow its footprint (PROT entry {idx})")
        })?;
        writes.push((idx, scene.man_offset as u64, stream));
    }
    for (idx, offset, stream) in &writes {
        patcher
            .patch_prot_entry(*idx, *offset, stream)
            .with_context(|| format!("write Fortress scene MAN (PROT entry {idx})"))?;
    }
    Ok(FortressEncounterFixReport {
        scenes_changed: writes.len(),
    })
}
