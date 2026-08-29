//! Nivora Ravine (`nilboa`) field-mesh mirror of `--delilas-party`: make
//! the duel scene's three Delilas NPC field meshes wear the mapped
//! **heroes'** field models, so the scene shows Vahn / Noa / Gala facing
//! the swapped party instead of a second set of Delilas siblings.
//!
//! Thin disc adapter over [`legaia_asset::party_swap::nivora_field`]:
//! reads the scene TMD pack (PROT 0639) + scene bundle (PROT 0638),
//! rebuilds both same-size, and writes them back in place.
//!
//! **Ordering constraint (load-bearing):** the caller must capture the
//! PROT 0874 entry bytes **before** `apply_delilas_party` runs - that
//! pass rewrites 0874 with sibling geometry (`party_swap::fieldize`),
//! and this pass sources the heroes' retail field meshes from it.
//! Like the fieldize pass itself, run this only alongside a **fresh**
//! swap apply: it is byte-deterministic but not self-detecting, so a
//! second run over an already-heroized scene would bake garbage.

use anyhow::{Context, Result};
use legaia_asset::party_swap::nivora_field::{self, SlotReport};

use crate::delilas_party::PartyMapping;
use crate::disc::DiscPatcher;

/// Report of one [`apply_nivora_field`] run.
#[derive(Debug, Default)]
pub struct NivoraFieldReport {
    /// Human-readable notes (decimation level, texture downscales).
    pub notes: Vec<String>,
    /// Per-sibling verification numbers.
    pub slots: Vec<SlotReport>,
}

/// Swap the nilboa Delilas NPC field meshes for the mapped heroes'.
///
/// `prot_0874_retail` = the PROT 0874 entry footprint bytes captured
/// BEFORE the party swap's fieldize pass rewrote the entry (see the
/// module doc for why).
pub fn apply_nivora_field(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    prot_0874_retail: &[u8],
) -> Result<NivoraFieldReport> {
    let npc_pack = patcher
        .read_entry_footprint(nivora_field::NPC_PACK_ENTRY)
        .context("read PROT 0639 (nilboa TMD pack)")?;
    let npc_bundle = patcher
        .read_entry_footprint(nivora_field::NPC_BUNDLE_ENTRY)
        .context("read PROT 0638 (nilboa scene bundle)")?;
    let field_mapping = [
        mapping.vahn.monster_id(),
        mapping.noa.monster_id(),
        mapping.gala.monster_id(),
    ];
    let (patch, slots) =
        nivora_field::heroize_nilboa(&npc_pack, &npc_bundle, prot_0874_retail, field_mapping)
            .context("rebuild nilboa Delilas field meshes as the heroes")?;
    patcher
        .patch_prot_entry(nivora_field::NPC_PACK_ENTRY, 0, &patch.pack_entry)
        .context("write PROT 0639")?;
    patcher
        .patch_prot_entry(nivora_field::NPC_BUNDLE_ENTRY, 0, &patch.bundle_entry)
        .context("write PROT 0638")?;
    Ok(NivoraFieldReport {
        notes: patch
            .warnings
            .iter()
            .map(|w| format!("nilboa field: {w}"))
            .collect(),
        slots,
    })
}

/// Swap every OTHER Delilas event appearance (the map-stone
/// confrontation in `stone`, Zora's floating castle in `taiku2`, past
/// Conkram in `conc2`) for the mapped heroes' field meshes - the
/// scene-bundle sibling of [`apply_nivora_field`], driven by
/// [`legaia_asset::party_swap::event_field::EVENT_SCENES`].
///
/// Same ordering constraint as the nilboa pass: `prot_0874_retail` is
/// the pre-fieldize PROT 0874 entry bytes.
pub fn apply_event_field(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    prot_0874_retail: &[u8],
) -> Result<NivoraFieldReport> {
    use legaia_asset::party_swap::event_field;

    let field_mapping = [
        mapping.vahn.monster_id(),
        mapping.noa.monster_id(),
        mapping.gala.monster_id(),
    ];
    let mut report = NivoraFieldReport::default();
    for spec in event_field::EVENT_SCENES {
        let bundle = patcher
            .read_entry_footprint(spec.bundle_entry)
            .with_context(|| format!("read PROT {} ({} bundle)", spec.bundle_entry, spec.scene))?;
        let tim_entry = spec
            .tim_entry
            .map(|e| {
                patcher
                    .read_entry_footprint(e)
                    .with_context(|| format!("read PROT {e} ({} TIM pack)", spec.scene))
            })
            .transpose()?;
        let (patch, slots) = event_field::heroize_event_scene(
            spec,
            &bundle,
            tim_entry.as_deref(),
            prot_0874_retail,
            field_mapping,
        )
        .with_context(|| format!("rebuild {} Delilas field meshes as the heroes", spec.scene))?;
        patcher
            .patch_prot_entry(spec.bundle_entry, 0, &patch.bundle_entry)
            .with_context(|| format!("write PROT {}", spec.bundle_entry))?;
        if let (Some(entry), Some(bytes)) = (spec.tim_entry, patch.tim_entry.as_ref()) {
            patcher
                .patch_prot_entry(entry, 0, bytes)
                .with_context(|| format!("write PROT {entry}"))?;
        }
        report.notes.extend(
            patch
                .warnings
                .iter()
                .map(|w| format!("{} field: {w}", spec.scene)),
        );
        report.slots.extend(slots);
    }
    Ok(report)
}
