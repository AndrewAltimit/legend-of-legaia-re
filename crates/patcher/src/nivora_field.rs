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
