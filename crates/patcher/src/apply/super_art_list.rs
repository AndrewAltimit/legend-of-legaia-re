//! Show Super Arts on the in-battle Tactical-Arts list. See
//! [`crate::super_art_list`] for the four hook sites and why the names are
//! carried in dead space rather than chased through RAM.

use super::*;

/// Outcome of enabling the Super Arts list rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperArtListReport {
    /// The fifteen names written into the SCUS name blob, in
    /// `character * 5 + k` order.
    pub names: Vec<String>,
    /// Where the injected routines landed (for the oracle / manifest).
    pub count_va: u32,
    pub id_va: u32,
    pub draw_va: u32,
    pub blob_va: u32,
}

/// Inject the **show Super Arts** feature: add each of Vahn's, Noa's and Gala's
/// five Super Arts to the arts list the Triangle button opens in battle, which
/// retail draws not at all.
///
/// Three same-size detours into the SCUS list renderer `FUN_80034358` plus their
/// routines and a name blob in verified-dead SCUS regions, and a wholesale
/// in-place replacement of the list pager `FUN_801D3748` in the battle-action
/// overlay (PROT 0898) so the page offset can reach the added rows.
///
/// **Mutually exclusive with `--shiny-seru`, `--arts-ap-grant` / `--arts-ap-cost`
/// and `--delilas-challenge`** - they all reuse the same arena bytes. Fails
/// (without touching the disc) if the build isn't the recognized US layout or a
/// hosted region isn't dead space.
pub fn inject_super_art_list(patcher: &mut DiscPatcher) -> Result<SuperArtListReport> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for show-super-arts injection")?;
    let ov0898 = patcher
        .read_entry(crate::super_art_list::OVERLAY_PROT_INDEX)
        .context("read battle-action overlay (0898) for show-super-arts injection")?;
    let plan = crate::super_art_list::SuperArtListInjection::plan(&scus, &ov0898)?;

    for edit in &plan.edits {
        match edit.prot_index {
            None => patcher
                .patch_named_file(SCUS_NAME, edit.file_off as u64, &edit.bytes)
                .with_context(|| {
                    format!("write show-super-arts SCUS edit at {:#x}", edit.file_off)
                })?,
            Some(idx) => patcher
                .patch_prot_entry(idx, edit.file_off as u64, &edit.bytes)
                .with_context(|| {
                    format!(
                        "write show-super-arts PROT {idx} edit at {:#x}",
                        edit.file_off
                    )
                })?,
        }
    }

    Ok(SuperArtListReport {
        names: plan.names,
        count_va: plan.count_va,
        id_va: plan.id_va,
        draw_va: plan.draw_va,
        blob_va: plan.blob_va,
    })
}
