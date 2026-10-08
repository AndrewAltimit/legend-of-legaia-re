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
///
/// The pinned tile and model are not enough on their own: the later Rim Elm
/// variants (`town0b` / `town0c` / `town0d`) place Tetsu on the same tile
/// with the same model, and only `town01`'s record carries the fight
/// (`3E FF 04` in `P1[10]`). The carrier is installed only when the
/// placement's own record names the training row in a scripted-battle op
/// ([`record_battle_entry_rows`]) - the only way retail enters the fight -
/// so talking to Tetsu in the mist-attack town stays a conversation.
pub fn derive_field_carriers(man_file: &ManFile, man: &[u8]) -> Vec<DerivedFieldCarrier> {
    let training_row = crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID as u8;
    classify_placements(man_file, man)
        .into_iter()
        .filter_map(|(p, kind)| {
            let config = if is_rim_elm_sparring_carrier(&p)
                && record_battle_entry_rows(man_file, man, 1, p.index).contains(&training_row)
            {
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

/// Per-field-carrier role. The retail engine builds one record per MAN-placed
/// scene entity; this is the port's slice the field entity SM acts on.
/// Paired by index with `legaia_engine_core::world::FieldCarrierState::entities`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldCarrierConfig {
    /// A **scripted-encounter carrier**: engaging it (the dialogue-accept)
    /// advances its `FUN_801DA51C` SM to its scene-transition, which selects
    /// MAN formation `formation_id` (by index, so the scene's merged monster
    /// stats stand) and launches the battle. The Rim Elm Tetsu tutorial fight
    /// is `formation_id` [`crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID`].
    ScriptedEncounter { formation_id: u16 },
    /// A plain interactable NPC. Surfaces a
    /// [`crate::field_events::FieldEvent::FieldInteract`] with `interact_id`.
    Npc { interact_id: u8 },
}
