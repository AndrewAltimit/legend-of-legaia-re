//! Derived field-carrier menus (Rim Elm sparring carrier detection).
//!
//! Extracted verbatim from `man_field_scripts.rs`.

use super::*;

/// `true` when `p` is the Rim Elm sparring partner: the partition-1 placement
/// pinned at [`RIM_ELM_SPARRING_CARRIER_TILE`] carrying
/// [`RIM_ELM_SPARRING_CARRIER_MODEL`] (the NPC whose talk-menu installs the
/// opening lone-Tetsu training fight). See [`crate::encounter_record`].
pub fn is_rim_elm_sparring_carrier(p: &ActorPlacement) -> bool {
    (p.tile_x, p.tile_z) == crate::encounter_record::RIM_ELM_SPARRING_CARRIER_TILE
        && p.model_index == crate::encounter_record::RIM_ELM_SPARRING_CARRIER_MODEL
}

/// A field carrier derived from one MAN partition-1 placement: the placement it
/// came from plus the [`FieldCarrierConfig`] its identity / script implies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedFieldCarrier {
    /// Partition-1 record index of the source placement (retail actor record).
    pub placement_index: usize,
    /// Source placement tile (column, row).
    pub tile: (u8, u8),
    /// Source placement model byte.
    pub model: u8,
    /// The carrier role to install for this placement.
    pub config: FieldCarrierConfig,
}

/// Derive field-carrier configs **directly from a scene MAN's actor
/// placements**, instead of hand-building them.
///
/// Each interactable placement ([`PlacementKind::Npc`]) becomes a carrier:
///
/// - the pinned Rim Elm sparring partner ([`is_rim_elm_sparring_carrier`]) maps
///   to [`FieldCarrierConfig::ScriptedEncounter`] for the training formation
///   ([`crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID`]);
/// - every other talk-to NPC maps to [`FieldCarrierConfig::Npc`] keyed by its
///   partition-1 record index (the retail interaction-script selector).
///
/// Decorative ([`PlacementKind::Plain`]) and warp ([`PlacementKind::Portal`])
/// placements carry no engageable carrier SM and are skipped; each
/// [`DerivedFieldCarrier`] keeps its `placement_index` so a caller can map a
/// carrier-Vec index back to the MAN actor.
///
/// The formation **index** the sparring carrier launches (`= 4`) is still a
/// pinned constant: a town01 field interaction record selects its formation by
/// index, not via an inline `[count][ids]` literal (proven by the partition-1
/// script walk), so the selection bytecode is not yet decoded. What this
/// derives from the MAN is the carrier's *identity and placement* - which actor
/// is the carrier, where it stands, and that the scene actually contains it -
/// rather than fabricating a standalone carrier with no MAN linkage.
pub fn derive_field_carriers(man_file: &ManFile, man: &[u8]) -> Vec<DerivedFieldCarrier> {
    classify_placements(man_file, man)
        .into_iter()
        .filter_map(|(p, kind)| {
            let config = if is_rim_elm_sparring_carrier(&p) {
                FieldCarrierConfig::ScriptedEncounter {
                    formation_id: crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID,
                }
            } else if matches!(kind, PlacementKind::Npc { .. }) {
                FieldCarrierConfig::Npc {
                    interact_id: p.index as u8,
                }
            } else {
                return None;
            };
            Some(DerivedFieldCarrier {
                placement_index: p.index,
                tile: (p.tile_x, p.tile_z),
                model: p.model_index,
                config,
            })
        })
        .collect()
}
