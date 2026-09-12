//! Enemy HP bars. See [`crate::enemy_hp_bar`] for the routine, its four
//! unreferenced host bodies and the hook site.

use super::*;

/// Outcome of enabling enemy HP bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyHpBarReport {
    /// Words of routine written into the battle-action overlay (PROT 0898).
    pub overlay_words: usize,
    /// Words of routine written into `SCUS_942.54`.
    pub scus_words: usize,
    /// Same-size edits written.
    pub edits: usize,
}

/// Draw a **red HP gauge over every living monster** in battle: the `HP`
/// label chip and the AP meter's gauge primitive (percentage fill + numeral),
/// fed by the displayed-HP mirror so it steps hit by hit.
///
/// Five same-size edits: a two-word detour at the head of the damage-number
/// popup renderer in the battle-action overlay, and the routine itself laid
/// over four routines retail never references (three in PROT 0898, one in
/// `SCUS_942.54`). Claims no injected-code arena bytes, so it composes with
/// every other code hook. Fails (without touching the disc) if the build is
/// not the recognized US layout or any host body is no longer retail's.
pub fn inject_enemy_hp_bar(patcher: &mut DiscPatcher) -> Result<EnemyHpBarReport> {
    let scus = patcher
        .read_named_file(SCUS_NAME)
        .context("read SCUS_942.54 for enemy HP bars")?;
    let overlay = patcher
        .read_entry(crate::enemy_hp_bar::OVERLAY_PROT_INDEX)
        .context("read battle-action overlay (0898) for enemy HP bars")?;
    let plan = crate::enemy_hp_bar::EnemyHpBarInjection::plan(&scus, &overlay)?;
    for edit in &plan.edits {
        match edit.prot_index {
            None => patcher
                .patch_named_file(SCUS_NAME, edit.file_off as u64, &edit.bytes)
                .with_context(|| format!("write enemy-HP-bar SCUS edit at {:#x}", edit.file_off))?,
            Some(idx) => patcher
                .patch_prot_entry(idx, edit.file_off as u64, &edit.bytes)
                .with_context(|| {
                    format!("write enemy-HP-bar PROT {idx} edit at {:#x}", edit.file_off)
                })?,
        }
    }
    Ok(EnemyHpBarReport {
        overlay_words: plan.overlay_words,
        scus_words: plan.scus_words,
        edits: plan.edits.len(),
    })
}
