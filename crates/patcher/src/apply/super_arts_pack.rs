//! Super Arts Pack, by ZetaPhoenix. See [`crate::super_arts_pack`] for the
//! block's layout, where its bytes live on the patched disc, and how each hook
//! was derived.

use super::*;

/// Outcome of installing the Super Arts Pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperArtsPackReport {
    /// The fifteen added Super Art names, in table order (five per character,
    /// Vahn then Noa then Gala).
    pub names: Vec<String>,
    /// Absolute disc LBA the 3764-byte block was parked at, and the sectors it
    /// took from the `DMY.DAT` annex.
    pub block_lba: u32,
    pub block_sectors: u32,
    /// Where the battle-load stub and the queue-hook trampoline landed.
    pub stub_va: u32,
    pub trampoline_va: u32,
    /// Same-size word edits written (excluding the annexed block itself).
    pub edits: usize,
}

/// Install the **Super Arts Pack by ZetaPhoenix**: fifteen extra Super Arts -
/// five per character on top of the retail five - each with its own name,
/// hit count and animation, triggered by their own arts chains.
///
/// ZetaPhoenix's 3764-byte block is parked in the `DMY.DAT` annex and streamed
/// to `0x801FD000` at battle load by an injected stub; ten same-size word edits
/// point the retail Super-Art applier, the arts queue builder and the two banner
/// routines at it. His bytes are installed unmodified.
///
/// **Mutually exclusive with `--shiny-seru`, `--show-super-arts`,
/// `--arts-ap-grant` / `--arts-ap-cost` and `--delilas-challenge`** - they all
/// host code in the same verified-dead SCUS arena. Fails without touching the
/// disc if the build is not the recognized US layout, the arena is not dead
/// space, the disc's own Super Art trigger rows differ from the pack's, or the
/// annex has no room.
pub fn inject_super_arts_pack(patcher: &mut DiscPatcher) -> Result<SuperArtsPackReport> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for the Super Arts Pack")?;
    let overlay = patcher
        .read_entry(crate::super_arts_pack::OVERLAY_PROT_INDEX)
        .context("read battle-action overlay (0898) for the Super Arts Pack")?;

    // Plan against a *hypothetical* LBA first, so a build/guard failure refuses
    // before an annex sector is spent (allocation is a one-way bump pointer).
    crate::super_arts_pack::SuperArtsPackInjection::plan(&scus, &overlay, 1)
        .context("Super Arts Pack (by ZetaPhoenix)")?;

    let blob = crate::super_arts_pack::block_sectors();
    let (block_lba, block_sectors) = patcher
        .annex_blob(&blob)
        .context("park the Super Arts Pack block in the DMY.DAT annex")?;

    let plan = crate::super_arts_pack::SuperArtsPackInjection::plan(&scus, &overlay, block_lba)
        .context("Super Arts Pack (by ZetaPhoenix)")?;
    for edit in &plan.edits {
        match edit.prot_index {
            None => patcher
                .patch_named_file(SCUS_NAME, edit.file_off as u64, &edit.bytes)
                .with_context(|| {
                    format!("write Super Arts Pack SCUS edit at {:#x}", edit.file_off)
                })?,
            Some(idx) => patcher
                .patch_prot_entry(idx, edit.file_off as u64, &edit.bytes)
                .with_context(|| {
                    format!(
                        "write Super Arts Pack PROT {idx} edit at {:#x}",
                        edit.file_off
                    )
                })?,
        }
    }

    Ok(SuperArtsPackReport {
        names: plan.names,
        block_lba,
        block_sectors,
        stub_va: plan.stub_va,
        trampoline_va: plan.trampoline_va,
        edits: plan.edits.len(),
    })
}
